//! The Fastly implementation of the [`Platform`] SPI (SPEC §8.2).
//!
//! One `FastlyPlatform` is constructed per request instance, so the dynamic
//! backend cache is naturally per-session (SPEC §7.3.2). Every operation
//! performs blocking host calls and resolves on the first poll (SPEC §8.3).

use std::fmt;
use std::io::Write;
use std::sync::Mutex;

use edge_core::{
    client::{ClientMetadata, EdgeProvider, GeoMetadata, NetworkMetadata, TlsMetadata},
    config::EdgeConfig,
    context::{Context, LogLevel, Platform},
    error::{Error, FetchError, KvError},
    kv::KvStore,
    log::LogFieldMap,
    types::EdgeRequest,
    EdgeResponse, Result,
};
use fastly::{config_store::ConfigStore, log::Endpoint, secret_store::SecretStore, KVStore};
use futures_util::future::BoxFuture;

use crate::convert;
use crate::kv as fastly_kv;
use crate::resolve::{self, BackendCache};

/// Map a `fastly::SendError` to the normalized error model (SPEC §6.4).
fn map_send_error(e: &fastly::http::request::SendError) -> FetchError {
    use fastly::http::request::SendErrorCause as C;
    match e.root_cause() {
        C::DnsTimeout | C::ConnectionTimeout | C::HttpResponseTimeout => FetchError::Timeout,
        C::TlsProtocolError
        | C::TlsCertificateError
        | C::TlsAlertReceived { .. }
        | C::TlsConfigurationError => FetchError::Tls(e.to_string()),
        C::DnsError { .. }
        | C::DestinationNotFound
        | C::DestinationUnavailable
        | C::DestinationIpUnroutable
        | C::ConnectionRefused
        | C::ConnectionTerminated
        | C::ConnectionLimitReached
        | C::HttpIncompleteResponse
        | C::HttpResponseHeaderSectionTooLarge
        | C::HttpResponseBodyTooLarge
        | C::HttpResponseStatusInvalid
        | C::HttpUpgradeFailed
        | C::Http2StreamError { .. }
        | C::HttpProtocolError
        | C::HttpRequestCacheKeyInvalid
        | C::HttpRequestUriInvalid
        | C::HttpCacheLimitExceeded
        | C::HttpCacheApiUnsupported
        | C::IoError(_)
        | C::ImageOptimizerUnsupported
        | C::InternalError(_)
        | C::RequestCollapse
        | C::Custom(_) => FetchError::Connection(e.to_string()),
        // `SendErrorCause` is non_exhaustive; keep unknown causes lossless
        // under the platform category.
        _ => FetchError::Platform(format!("fastly: {e}")),
    }
}

/// The Fastly platform.
pub struct FastlyPlatform {
    config: EdgeConfig,
    config_store: Option<ConfigStore>,
    secret_store: Option<SecretStore>,
    log_endpoint: Mutex<Option<Endpoint>>,
    backends: BackendCache,
    log_fields: Mutex<LogFieldMap>,
}

impl fmt::Debug for FastlyPlatform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FastlyPlatform")
            .field("service", &self.config.service().name)
            .field("config_store", &self.config_store.is_some())
            .field("secret_store", &self.secret_store.is_some())
            .field("log_endpoint", &self.log_endpoint.lock().unwrap().is_some())
            .finish_non_exhaustive()
    }
}

impl FastlyPlatform {
    /// Build the Fastly-backed [`Context`] (SPEC §8.2).
    ///
    /// Missing stores degrade gracefully: `var`/`secret` return `None` and
    /// `kv()` fails with [`KvError::Platform`].
    pub fn context(config: EdgeConfig, metadata: ClientMetadata) -> Context {
        let platform = Self::new(config);
        Context::from_platform_with_metadata(Box::new(platform), metadata)
    }

    fn new(config: EdgeConfig) -> Self {
        let config_store = config
            .stores()
            .config
            .as_deref()
            .and_then(|name| ConfigStore::try_open(name).ok());
        let secret_store = config
            .stores()
            .secrets
            .as_deref()
            .and_then(|name| SecretStore::open(name).ok());
        let log_endpoint = config
            .logging()
            .endpoint
            .as_deref()
            .and_then(|name| Endpoint::try_from_name(name).ok());
        Self {
            config,
            config_store,
            secret_store,
            log_endpoint: Mutex::new(log_endpoint),
            backends: BackendCache::new(),
            log_fields: Mutex::new(LogFieldMap::new()),
        }
    }

    fn send_request(&self, req: EdgeRequest) -> Result<fastly::Response> {
        let backend = resolve::resolve(&self.config, req.uri(), &self.backends)?;
        let fastly_req = convert::to_fastly(req);
        fastly_req
            .send(backend)
            .map_err(|e| Error::Fetch(map_send_error(&e)))
    }

    fn fetch_blocking(&self, req: EdgeRequest) -> Result<EdgeResponse> {
        let resp = self.send_request(req)?;
        convert::from_fastly(resp)
    }

    fn fetch_blocking_streaming(&self, req: EdgeRequest) -> Result<EdgeResponse> {
        let resp = self.send_request(req)?;
        convert::from_fastly_streaming(resp)
    }
}

impl Platform for FastlyPlatform {
    fn fetch(&self, req: EdgeRequest) -> BoxFuture<'_, Result<EdgeResponse>> {
        // SPEC §8.3: the body performs blocking host calls only, so the
        // future resolves on its first poll and the drive loop terminates.
        Box::pin(async move { self.fetch_blocking(req) })
    }

    fn fetch_streaming(&self, req: EdgeRequest) -> BoxFuture<'_, Result<EdgeResponse>> {
        // Same host `send` as fetch — headers arrive first, the body handle
        // stays live — so this also resolves on the first poll (SPEC D21).
        Box::pin(async move { self.fetch_blocking_streaming(req) })
    }

    fn var(&self, name: &str) -> Option<String> {
        self.config_store.as_ref()?.try_get(name).ok().flatten()
    }

    fn secret(&self, name: &str) -> Option<Vec<u8>> {
        let secret = self.secret_store.as_ref()?.try_get(name).ok().flatten()?;
        Some(secret.plaintext().to_vec())
    }

    fn kv(&self, name: &str) -> Result<KvStore> {
        // v1 configures a single default store (SPEC §6.5): the `default`
        // handle resolves to the store bound in `edge.toml`.
        if name != edge_core::context::DEFAULT_KV_STORE {
            return Err(Error::Kv(KvError::Platform(format!(
                "no KV store bound to `{name}` (only `default` is configured in v1)"
            ))));
        }
        let store_name = self.config.stores().kv.as_deref().ok_or_else(|| {
            Error::Kv(KvError::Platform(
                "no KV store configured: set [stores] kv in edge.toml".to_string(),
            ))
        })?;
        let store = KVStore::open(store_name)
            .map_err(|e| Error::Kv(KvError::Platform(e.to_string())))?
            .ok_or_else(|| {
                Error::Kv(KvError::Platform(format!(
                    "KV store `{store_name}` not found (declare it in fastly.toml [setup.kv_store])"
                )))
            })?;
        Ok(fastly_kv::wrap(store))
    }

    fn log(&self, level: LogLevel, message: &str) {
        let line = format!("[{}] {}\n", level.as_str(), message);
        let mut endpoint = self
            .log_endpoint
            .lock()
            .expect("fastly log endpoint mutex poisoned");
        match endpoint.as_mut() {
            // SPEC §6.3: configured endpoint, falling back to stderr
            // (captured by Viceroy).
            Some(ep) => {
                let _ = ep.write_all(line.as_bytes());
            }
            None => {
                eprintln!("{}", line.trim_end());
            }
        }
    }

    fn set_log_field(&self, key: String, value: String) -> Result<()> {
        let mut map = self.log_fields.lock().expect("fastly log fields poisoned");
        let res = map.set(key, value);
        let diags = map.drain_diagnostics();
        drop(map);
        for d in diags {
            self.log(LogLevel::Warn, &d);
        }
        res
    }

    fn remove_log_field(&self, key: &str) {
        self.log_fields
            .lock()
            .expect("fastly log fields poisoned")
            .remove(key);
    }

    fn finalize_log_fields(&self) -> Option<String> {
        let mut map = self.log_fields.lock().expect("fastly log fields poisoned");
        let diags = map.drain_diagnostics();
        let record = map.serialize();
        drop(map);
        for d in diags {
            self.log(LogLevel::Warn, &d);
        }
        // One structured record per request, emitted at finalization (SPEC
        // PORTABILITY-PRIMITIVES §6): `{"fields": {...}}` to the configured
        // log endpoint, or the stderr fallback.
        if let Some(fields) = record {
            let line = format!("{{\"fields\":{fields}}}\n");
            let mut endpoint = self
                .log_endpoint
                .lock()
                .expect("fastly log endpoint mutex poisoned");
            match endpoint.as_mut() {
                Some(ep) => {
                    let _ = ep.write_all(line.as_bytes());
                }
                None => {
                    eprintln!("{}", line.trim_end());
                }
            }
        }
        // Fields never ride in the client response on Fastly; the record
        // above is the whole story.
        None
    }
}

/// Capture the downstream client metadata snapshot (SPEC-PORTABILITY-PRIMITIVES §5,
/// M10).
///
/// Sources (documented in the spec's minimum-mapping table):
///
/// - client IP: downstream client IP API (`get_client_ip_addr`);
/// - POP: `fastly::compute_runtime::pop()`;
/// - original header names: the downstream original-header API;
/// - geo/network: `fastly::geo::geo_lookup` on the client IP;
/// - TLS: JA3/JA4 from the downstream TLS metadata (protocol/cipher and
///   cipher/extension hashes are not exposed by the fastly 0.13 SDK — they
///   stay `None`).
///
/// Unavailable data is `None` — never substituted.
pub(crate) fn capture_client_metadata(req: &fastly::Request) -> ClientMetadata {
    let client_ip = req.get_client_ip_addr();

    // Fastly 0.13 exposes the POP via the compute-runtime API; Viceroy sets
    // FASTLY_POP. "--" / empty mean "unknown" (presentation is never
    // substituted, so those become `None`).
    let pop = {
        let p = fastly::compute_runtime::pop();
        if p.is_empty() || p == "--" {
            None
        } else {
            Some(p.to_string())
        }
    };

    // Original header names, preserving the order received. `Err`
    // (BufferSizeError) and `None` (not the client request) both mean
    // unavailable.
    let original_header_names = req
        .get_original_header_names()
        .ok()
        .flatten()
        .map(|names| names.to_vec());

    let geo_data = client_ip.and_then(fastly::geo::geo_lookup);
    let geo = geo_data
        .as_ref()
        .map(|g| GeoMetadata {
            continent: empty_as_none(g.continent().as_code(), "??"),
            country_code: empty_as_none(g.country_code(), ""),
            region_code: g.region().map(str::to_string),
            city: empty_as_none(g.city(), ""),
            postal_code: empty_as_none(g.postal_code(), ""),
            metro_code: (g.metro_code() != 0).then_some(g.metro_code().to_string()),
            // Fastly returns 0.0 when coordinates are unavailable; treat the
            // equator-anchored sentinel as unavailable (no substitution).
            latitude: (g.latitude() != 0.0 || g.longitude() != 0.0).then_some(g.latitude()),
            longitude: (g.latitude() != 0.0 || g.longitude() != 0.0).then_some(g.longitude()),
        })
        .unwrap_or_default();
    let network = geo_data
        .as_ref()
        .map(|g| NetworkMetadata {
            asn: (g.as_number() != 0).then_some(g.as_number()),
            as_organization: empty_as_none(g.as_name(), ""),
            proxy_type: proxy_type_name(g.proxy_type()),
            proxy_description: proxy_description_name(g.proxy_description()),
        })
        .unwrap_or_default();

    let tls = TlsMetadata {
        protocol: None, // not exposed by the fastly 0.13 SDK
        cipher: None,   // not exposed by the fastly 0.13 SDK
        ja3: req.get_tls_ja3_md5().map(|md5| hex(&md5)),
        ja4: req.get_tls_ja4().map(str::to_string),
        ciphers_sha1: None,    // not exposed by the fastly 0.13 SDK
        extensions_sha1: None, // not exposed by the fastly 0.13 SDK
    };

    ClientMetadata {
        provider: EdgeProvider::Fastly,
        client_ip,
        pop,
        original_header_names,
        geo,
        network,
        tls,
    }
}

fn empty_as_none(value: &str, unknown: &str) -> Option<String> {
    if value.is_empty() || value == unknown {
        None
    } else {
        Some(value.to_string())
    }
}

fn hex(bytes: &[u8; 16]) -> String {
    let mut out = String::with_capacity(32);
    for b in bytes {
        use std::fmt::Write as _;
        let _ = write!(out, "{b:02x}");
    }
    out
}

/// Map a Fastly `ProxyType` to a stable name; `Unknown`/fallback is `None`.
fn proxy_type_name(t: fastly::geo::ProxyType) -> Option<String> {
    use fastly::geo::ProxyType::*;
    match t {
        Anonymous | Aol | Blackberry | Corporate | Edu | Hosting | Public | Transparent => {
            Some(format!("{t:?}"))
        }
        Unknown => None,
        Other(s) => Some(s),
        _ => None, // future variants: unavailable
    }
}

/// Map a Fastly `ProxyDescription` to a stable name; `Unknown`/fallback is
/// `None`.
fn proxy_description_name(d: fastly::geo::ProxyDescription) -> Option<String> {
    use fastly::geo::ProxyDescription::*;
    match d {
        Cloud | CloudSecurity | Dns | TorExit | TorRelay | Vpn | WebBrowser => {
            Some(format!("{d:?}"))
        }
        Unknown => None,
        Other(s) => Some(s),
        _ => None, // future variants: unavailable
    }
}
