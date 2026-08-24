//! The Cloudflare implementation of the [`Platform`] SPI (SPEC §8.1).
//!
//! `Env` provides all bindings: plaintext vars/secrets and KV namespaces.
//! `fetch` runs on the JS event loop (native async). Errors are mapped to
//! the normalized model (SPEC §6.4); note the asymmetry documented in
//! SPEC D16: JavaScript fetch rejections do not expose typed causes the way
//! Fastly's `SendErrorCause` does, so CF failures map to the `Connection`
//! category with the JS message.

use std::fmt;

use edge_core::{
    context::{Context, LogLevel, Platform},
    error::{Error, FetchError, KvError},
    kv::KvStore,
    types::EdgeRequest,
    EdgeResponse, Result,
};
use futures_util::future::BoxFuture;
use js_sys::futures::JsFuture;
use wasm_bindgen::JsCast;

use crate::convert;
use crate::kv::CloudflareKvBackend;
use crate::send::SendFuture;

/// Map a JS fetch rejection to the normalized error model (SPEC §6.4).
fn map_fetch_js_error(e: wasm_bindgen::JsValue) -> FetchError {
    // CF fetch failures are JS TypeErrors / Error objects; the runtime does
    // not expose typed causes (see module docs / SPEC D16).
    FetchError::Connection(convert::js_string(&e))
}

/// The Cloudflare platform.
pub struct CloudflarePlatform {
    env: worker::Env,
}

impl fmt::Debug for CloudflarePlatform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CloudflarePlatform").finish_non_exhaustive()
    }
}

impl CloudflarePlatform {
    /// Build the Cloudflare-backed [`Context`] (SPEC §8.1).
    pub fn context(env: worker::Env) -> Context {
        Context::from_platform(Box::new(Self::new(env)))
    }

    /// Wrap an `Env`.
    pub fn new(env: worker::Env) -> Self {
        Self { env }
    }

    async fn fetch_blocking(&self, req: EdgeRequest) -> Result<EdgeResponse> {
        // D5.2: redirect: manual — neither platform auto-follows.
        let (parts, body) = req.into_parts();
        let worker_body =
            convert::body_from_bytes(body).map_err(|e| Error::Internal(e.to_string()))?;
        let http_req: http::Request<worker::Body> = http::Request::from_parts(parts, worker_body);
        let promise = convert::fetch_request_manual(http_req)?;

        let ws_resp: web_sys::Response = JsFuture::from(promise)
            .await
            .map_err(map_fetch_js_error)
            .map_err(Error::Fetch)?
            .dyn_into()
            .map_err(|v| {
                Error::Fetch(FetchError::Platform(format!(
                    "cloudflare: fetch did not return a Response: {}",
                    convert::js_string(&v)
                )))
            })?;

        convert::response_to_edge(ws_resp).await
    }
}

impl Platform for CloudflarePlatform {
    fn fetch(&self, req: EdgeRequest) -> BoxFuture<'_, Result<EdgeResponse>> {
        // SendFuture: the JsFuture-based body is !Send (see `send` module).
        Box::pin(SendFuture(async move { self.fetch_blocking(req).await }))
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
        // `name` IS the KV binding name on Cloudflare (namespaces are
        // declared in wrangler.toml under that binding; SPEC §7.2).
        let store = self
            .env
            .kv(name)
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
}
