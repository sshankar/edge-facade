//! Host → backend transport mapping (SPEC §7.3).
//!
//! The resolution *decision* (static match / dynamic fallback / fail closed)
//! lives in `edge_core::config::Resolution`; this module maps it onto the
//! Fastly transport: `Backend::from_name` for static origins and
//! `Backend::builder` for the opt-in dynamic fallback (decision D4), with a
//! per-session cache (SPEC §7.3.2: dynamic backends are per-session
//! entities; names may overlap across sessions, so per-session caching is
//! correct).

use std::collections::HashMap;
use std::sync::Mutex;

use edge_core::{config::Resolution, FetchError};
use fastly::backend::{Backend, BackendCreationError};
use http::Uri;

/// Per-session cache of dynamic backends.
#[derive(Debug, Default)]
pub struct BackendCache(Mutex<HashMap<String, Backend>>);

impl BackendCache {
    /// Create an empty cache.
    pub fn new() -> Self {
        Self::default()
    }
}

/// Resolve the request URI's host to a backend and map transport failures
/// onto the normalized error model (SPEC §6.4).
pub fn resolve(
    config: &edge_core::EdgeConfig,
    uri: &Uri,
    cache: &BackendCache,
) -> Result<Backend, FetchError> {
    match config.resolve(uri)? {
        Resolution::Static(backend_name) => Backend::from_name(&backend_name)
            .map_err(|_| FetchError::UnresolvedBackend(uri_host(uri).to_string())),
        Resolution::Dynamic { host, port, https } => dynamic_backend(&host, port, https, cache),
        Resolution::Unresolved(host) => Err(FetchError::UnresolvedBackend(host)),
    }
}

fn dynamic_backend(
    host: &str,
    port: u16,
    https: bool,
    cache: &BackendCache,
) -> Result<Backend, FetchError> {
    let key = format!("{host}:{port}");

    if let Some(backend) = cache.0.lock().ok().and_then(|c| c.get(&key).cloned()) {
        return Ok(backend);
    }

    // SSL iff https (SNI = host); Host header override for parity (D5.1).
    let mut builder = Backend::builder(format!("edge_dynamic_{key}"), key.clone());
    builder = builder.override_host(host.to_string());
    if https {
        builder = builder.enable_ssl();
    }

    let backend = builder.finish().map_err(|e| match e {
        BackendCreationError::Disallowed => FetchError::Permission,
        other => FetchError::Platform(format!("fastly: dynamic backend `{key}`: {other}")),
    })?;

    let mut cache = cache
        .0
        .lock()
        .map_err(|_| FetchError::Platform("backend cache poisoned".to_string()))?;
    cache.insert(key, backend.clone());
    Ok(backend)
}

fn uri_host(uri: &Uri) -> &str {
    uri.host().unwrap_or_default()
}
