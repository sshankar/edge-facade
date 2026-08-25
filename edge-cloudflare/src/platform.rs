//! The Cloudflare implementation of the [`Platform`] SPI (SPEC §8.1).
//!
//! `Env` provides all bindings: plaintext vars/secrets and KV namespaces.
//! `fetch` runs on the JS event loop (native async). Errors are mapped to
//! the normalized model (SPEC §6.4); note the asymmetry documented in
//! SPEC D16: JavaScript fetch rejections do not expose typed causes the way
//! Fastly's `SendErrorCause` does, so CF failures map to the `Connection`
//! category with the JS message.

use std::fmt;
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
use futures_util::future::BoxFuture;
use js_sys::futures::JsFuture;
use wasm_bindgen::JsCast;
use worker_sys::ext::RequestExt as _;

use crate::convert;
use crate::kv::CloudflareKvBackend;
use crate::send::SendFuture;
use crate::worker_sys;

/// Map a JS fetch rejection to the normalized error model (SPEC §6.4).
fn map_fetch_js_error(e: wasm_bindgen::JsValue) -> FetchError {
    // CF fetch failures are JS TypeErrors / Error objects; the runtime does
    // not expose typed causes (see module docs / SPEC D16).
    FetchError::Connection(convert::js_string(&e))
}

/// The Cloudflare platform.
pub struct CloudflarePlatform {
    env: worker::Env,
    config: EdgeConfig,
    log_fields: Mutex<LogFieldMap>,
}

impl fmt::Debug for CloudflarePlatform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CloudflarePlatform")
            .field("service", &self.config.service().name)
            .finish_non_exhaustive()
    }
}

impl CloudflarePlatform {
    /// Build the Cloudflare-backed [`Context`] (SPEC §8.1), with the
    /// request's client metadata snapshot (M10).
    pub fn context(env: worker::Env, config: EdgeConfig, metadata: ClientMetadata) -> Context {
        Context::from_platform_with_metadata(Box::new(Self::new(env, config)), metadata)
    }

    /// Wrap an `Env` with the service's embedded config.
    pub fn new(env: worker::Env, config: EdgeConfig) -> Self {
        Self {
            env,
            config,
            log_fields: Mutex::new(LogFieldMap::new()),
        }
    }

    async fn fetch_blocking(&self, req: EdgeRequest) -> Result<EdgeResponse> {
        let ws_resp = self.fetch_ws_response(req).await?;
        convert::response_to_edge(ws_resp).await
    }

    async fn fetch_blocking_streaming(&self, req: EdgeRequest) -> Result<EdgeResponse> {
        let ws_resp = self.fetch_ws_response(req).await?;
        convert::response_to_edge_streaming(ws_resp)
    }

    /// Issue the fetch (D5.2: `redirect: manual`) and return the resolved
    /// `web_sys::Response` — headers only; the body is still streaming.
    async fn fetch_ws_response(&self, req: EdgeRequest) -> Result<web_sys::Response> {
        let promise = convert::fetch_request_manual(req)?;

        JsFuture::from(promise)
            .await
            .map_err(map_fetch_js_error)
            .map_err(Error::Fetch)?
            .dyn_into()
            .map_err(|v| {
                Error::Fetch(FetchError::Platform(format!(
                    "cloudflare: fetch did not return a Response: {}",
                    convert::js_string(&v)
                )))
            })
    }
}

impl Platform for CloudflarePlatform {
    fn fetch(&self, req: EdgeRequest) -> BoxFuture<'_, Result<EdgeResponse>> {
        // SendFuture: the JsFuture-based body is !Send (see `send` module).
        Box::pin(SendFuture(async move { self.fetch_blocking(req).await }))
    }

    fn fetch_streaming(&self, req: EdgeRequest) -> BoxFuture<'_, Result<EdgeResponse>> {
        Box::pin(SendFuture(async move {
            self.fetch_blocking_streaming(req).await
        }))
    }

    fn var(&self, name: &str) -> Option<String> {
        // Missing binding → None (T7: "None otherwise").
        self.env.var(name).ok().map(|v| v.to_string())
    }

    fn secret(&self, name: &str) -> Option<Vec<u8>> {
        self.env
            .secret(name)
            .ok()
            .map(|s| s.to_string().into_bytes())
    }

    fn kv(&self, name: &str) -> Result<KvStore> {
        // The `default` handle (`Context::kv`) resolves to the binding named
        // in edge.toml `[stores] kv` (SPEC §8.1: binding name from embedded
        // config — same semantics as the Fastly adapter); named handles map
        // straight to their binding name (multi-store services).
        let binding = if name == edge_core::context::DEFAULT_KV_STORE {
            self.config.stores().kv.as_deref().ok_or_else(|| {
                Error::Kv(KvError::Platform(
                    "no KV store configured: set [stores] kv in edge.toml".to_string(),
                ))
            })?
        } else {
            name
        };
        let store = self
            .env
            .kv(binding)
            .map_err(|e| Error::Kv(KvError::Platform(e.to_string())))?;
        Ok(KvStore::from_backend(Box::new(CloudflareKvBackend::new(
            store,
        ))))
    }

    fn log(&self, level: LogLevel, message: &str) {
        match level {
            LogLevel::Info => worker::console_log!("[{}] {}", level.as_str(), message),
            LogLevel::Warn => worker::console_warn!("[{}] {}", level.as_str(), message),
            LogLevel::Error => worker::console_error!("[{}] {}", level.as_str(), message),
        }
    }

    fn set_log_field(&self, key: String, value: String) -> Result<()> {
        let mut map = self.log_fields.lock().expect("cf log fields poisoned");
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
            .expect("cf log fields poisoned")
            .remove(key);
    }

    fn finalize_log_fields(&self) -> Option<String> {
        let mut map = self.log_fields.lock().expect("cf log fields poisoned");
        let diags = map.drain_diagnostics();
        let serialized = map.serialize();
        drop(map);
        for d in diags {
            self.log(LogLevel::Warn, &d);
        }
        // The serialized control-header value is inserted into the response
        // by the adapter (convert::apply_control_header); Cloudflare has no
        // out-of-band log endpoint, so the header is the boundary record.
        serialized
    }
}

/// Capture the downstream client metadata snapshot (SPEC-PORTABILITY-PRIMITIVES §5,
/// M10).
///
/// Sources (documented in the spec's minimum-mapping table):
///
/// - client IP: the `cf-connecting-ip` request header (Cloudflare's
///   connecting-client address);
/// - POP / geo / network / TLS: the request's `cf` properties (`request.cf`
///   — under workerd, injected via the `cf-blob` header, see the
///   conformance harness);
/// - original header names: not exposed by Cloudflare — always `None`
///   (never reconstructed, P8).
///
/// Unavailable data is `None` — never substituted.
pub(crate) fn capture_client_metadata(req: &web_sys::Request) -> ClientMetadata {
    let headers = req.headers();

    let client_ip = headers
        .get("cf-connecting-ip")
        .ok()
        .flatten()
        .and_then(|v| v.parse().ok());

    let cf = req.cf();
    let pop = cf
        .as_ref()
        .and_then(|cf| cf.colo().ok())
        .filter(|s| !s.is_empty());

    let geo = GeoMetadata {
        continent: cf.as_ref().and_then(|cf| cf.continent().ok().flatten()),
        country_code: cf.as_ref().and_then(|cf| cf.country().ok().flatten()),
        region_code: cf.as_ref().and_then(|cf| cf.region_code().ok().flatten()),
        city: cf.as_ref().and_then(|cf| cf.city().ok().flatten()),
        postal_code: cf.as_ref().and_then(|cf| cf.postal_code().ok().flatten()),
        metro_code: cf.as_ref().and_then(|cf| cf.metro_code().ok().flatten()),
        latitude: cf
            .as_ref()
            .and_then(|cf| cf.latitude().ok().flatten())
            .and_then(|v| v.parse().ok()),
        longitude: cf
            .as_ref()
            .and_then(|cf| cf.longitude().ok().flatten())
            .and_then(|v| v.parse().ok()),
    };
    let network = NetworkMetadata {
        asn: cf.as_ref().and_then(|cf| cf.asn().ok().flatten()),
        as_organization: cf
            .as_ref()
            .and_then(|cf| cf.as_organization().ok().flatten()),
        // Cloudflare's public API does not expose proxy classification.
        proxy_type: None,
        proxy_description: None,
    };
    let tls = TlsMetadata {
        protocol: cf
            .as_ref()
            .and_then(|cf| cf.tls_version().ok())
            .filter(|s| !s.is_empty()),
        cipher: cf
            .as_ref()
            .and_then(|cf| cf.tls_cipher().ok())
            .filter(|s| !s.is_empty()),
        // JA3/JA4 and cipher/extension hashes are not exposed by the public
        // API (Bot Management only).
        ja3: None,
        ja4: None,
        ciphers_sha1: None,
        extensions_sha1: None,
    };

    ClientMetadata {
        provider: EdgeProvider::Cloudflare,
        client_ip,
        pop,
        // Cloudflare does not expose original header names; never
        // reconstructed (P8).
        original_header_names: None,
        geo,
        network,
        tls,
    }
}
