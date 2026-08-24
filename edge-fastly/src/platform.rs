//! The Fastly implementation of the [`Platform`] SPI (SPEC §8.2).
//!
//! One `FastlyPlatform` is constructed per request instance, so the dynamic
//! backend cache is naturally per-session (SPEC §7.3.2). Every operation
//! performs blocking host calls and resolves on the first poll (SPEC §8.3).

use std::fmt;
use std::io::Write;
use std::sync::Mutex;

use edge_core::{
    config::EdgeConfig,
    context::{Context, LogLevel, Platform},
    error::{Error, FetchError, KvError},
    kv::KvStore,
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
    pub fn context(config: EdgeConfig) -> Context {
        let platform = Self::new(config);
        Context::from_platform(Box::new(platform))
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
}
