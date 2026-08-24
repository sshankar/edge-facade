//! The platform capability interface (`Platform` SPI) and the user-facing
//! [`Context`].

use std::fmt;
use std::sync::Arc;

use futures_util::future::BoxFuture;

use crate::kv::KvStore;
use crate::types::{EdgeRequest, EdgeResponse};
use crate::Result;

/// The name of the default KV store handle (`Context::kv`).
///
/// Adapters resolve this handle to the binding/store named in `edge.toml`
/// `[stores] kv` (SPEC §6.5, §8.1); named handles map directly.
pub const DEFAULT_KV_STORE: &str = "default";

/// Log severity levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LogLevel {
    /// Informational messages.
    Info,
    /// Recoverable problems.
    Warn,
    /// Fatal problems.
    Error,
}

impl LogLevel {
    /// The level's name, lowercase.
    pub fn as_str(&self) -> &'static str {
        match self {
            LogLevel::Info => "info",
            LogLevel::Warn => "warn",
            LogLevel::Error => "error",
        }
    }
}

/// Internal platform SPI, implemented by each platform adapter and by the
/// native mock (`edge_core::testing`).
///
/// **Not part of the stable public API** (`#[doc(hidden)]`): its shape may
/// change between minor versions. Adapter crates implement this trait and
/// construct a [`Context`] via [`Context::from_platform`].
#[doc(hidden)]
pub trait Platform: Send + Sync {
    /// Perform a subrequest. The request URI must be absolute.
    fn fetch(&self, req: EdgeRequest) -> BoxFuture<'_, Result<EdgeResponse>>;

    /// Look up a configuration variable by name.
    fn var(&self, name: &str) -> Option<String>;

    /// Look up a secret by name.
    fn secret(&self, name: &str) -> Option<Vec<u8>>;

    /// Open a KV store by binding name.
    fn kv(&self, name: &str) -> Result<KvStore>;

    /// Emit a log message at the given level.
    fn log(&self, level: LogLevel, message: &str);
}

/// The platform handle passed to every handler.
///
/// `Context` is cheap to clone (it wraps an `Arc`); all capabilities are
/// accessed through its methods.
#[derive(Clone)]
pub struct Context(Arc<dyn Platform>);

impl fmt::Debug for Context {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Context { .. }")
    }
}

impl Context {
    /// Wrap a platform implementation (adapter SPI; `#[doc(hidden)]`).
    #[doc(hidden)]
    pub fn from_platform(platform: Box<dyn Platform>) -> Self {
        Self(Arc::from(platform))
    }

    /// Perform a subrequest to an absolute URL (SPEC §7).
    ///
    /// The URI of `req` must be absolute; adapters resolve its host to a
    /// backend. Fails with [`FetchError::BadRequest`](crate::FetchError::BadRequest)
    /// if the URI is relative.
    pub async fn fetch(&mut self, req: EdgeRequest) -> Result<EdgeResponse> {
        self.0.fetch(req).await
    }

    /// Look up a configuration variable, or `None` if unset.
    pub fn var(&self, name: &str) -> Option<String> {
        self.0.var(name)
    }

    /// Look up a secret, or `None` if unset.
    pub fn secret(&self, name: &str) -> Option<Vec<u8>> {
        self.0.secret(name)
    }

    /// Open the default KV store.
    ///
    /// # Panics
    ///
    /// Panics if the platform does not provide a store bound to `"default"`.
    pub fn kv(&self) -> KvStore {
        self.kv_named(DEFAULT_KV_STORE)
            .expect("platform must provide a KV store bound to `default`")
    }

    /// Open a KV store by binding name.
    pub fn kv_named(&self, name: &str) -> Result<KvStore> {
        self.0.kv(name)
    }

    /// Emit a log message. Prefer the `edge_core::log::{info,warn,error}!`
    /// macros.
    pub fn log(&self, level: LogLevel, message: &str) {
        self.0.log(level, message);
    }
}
