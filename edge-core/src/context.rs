//! The platform capability interface (`Platform` SPI) and the user-facing
//! [`Context`].

use std::fmt;
use std::sync::Arc;

use futures_util::future::BoxFuture;

use crate::client::ClientMetadata;
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
    ///
    /// Returns a response whose body is fully buffered (v1 semantics, SPEC
    /// D2).
    fn fetch(&self, req: EdgeRequest) -> BoxFuture<'_, Result<EdgeResponse>>;

    /// Perform a subrequest, returning its body as a stream (SPEC D21).
    ///
    /// Returns once response headers are available; the body is a
    /// [`Body::Streaming`] source (never pre-buffered), so large payloads
    /// can be relayed or processed incrementally. Defaults to [`Platform::fetch`]
    /// (buffered); adapters override it. On Fastly this is the same host
    /// `send` call — headers arrive first and the body handle streams — so
    /// it resolves on the first poll like [`Platform::fetch`] (SPEC §8.3).
    fn fetch_streaming(&self, req: EdgeRequest) -> BoxFuture<'_, Result<EdgeResponse>> {
        self.fetch(req)
    }

    /// Look up a configuration variable by name.
    fn var(&self, name: &str) -> Option<String>;

    /// Look up a secret by name.
    fn secret(&self, name: &str) -> Option<Vec<u8>>;

    /// Open a KV store by binding name.
    fn kv(&self, name: &str) -> Result<KvStore>;

    /// Emit a log message at the given level.
    fn log(&self, level: LogLevel, message: &str);

    /// Set a structured log field (SPEC-PORTABILITY-PRIMITIVES §6).
    ///
    /// The adapter applies the shared policy from
    /// [`crate::log::LogFieldMap`] (normalization, budgets, diagnostics).
    fn set_log_field(&self, key: String, value: String) -> Result<()>;

    /// Remove a structured log field (a no-op if absent).
    fn remove_log_field(&self, key: &str);

    /// Finalize structured log fields at the adapter boundary.
    ///
    /// - Fastly: emits the structured record to the configured log endpoint
    ///   (or the stderr fallback) and returns `None`.
    /// - Cloudflare: returns the serialized control-header value (the
    ///   adapter inserts it into the response).
    /// - Mock: records the finalized snapshot for the harness and returns
    ///   the serialized control value.
    ///
    /// Runs for every request outcome — successful, synthetic, timeout, and
    /// catch-all responses (SPEC-PORTABILITY-PRIMITIVES §6).
    fn finalize_log_fields(&self) -> Option<String>;
}

/// The platform handle passed to every handler.
///
/// `Context` is cheap to clone (it wraps an `Arc`); all capabilities are
/// accessed through its methods.
#[derive(Clone)]
pub struct Context {
    platform: Arc<dyn Platform>,
    metadata: ClientMetadata,
}

impl fmt::Debug for Context {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Context")
            .field("provider", &self.metadata.provider)
            .finish_non_exhaustive()
    }
}

impl Context {
    /// Wrap a platform implementation (adapter SPI; `#[doc(hidden)]`).
    ///
    /// The resulting context reports an empty [`ClientMetadata`] with
    /// [`EdgeProvider::Mock`]; adapters must use
    /// [`Context::from_platform_with_metadata`].
    #[doc(hidden)]
    pub fn from_platform(platform: Box<dyn Platform>) -> Self {
        Self::from_platform_with_metadata(platform, ClientMetadata::default())
    }

    /// Wrap a platform implementation with the request's client metadata
    /// snapshot (adapter SPI; `#[doc(hidden)]`).
    #[doc(hidden)]
    pub fn from_platform_with_metadata(
        platform: Box<dyn Platform>,
        metadata: ClientMetadata,
    ) -> Self {
        Self {
            platform: Arc::from(platform),
            metadata,
        }
    }

    /// The client metadata snapshot captured at request entry
    /// (SPEC-PORTABILITY-PRIMITIVES §5).
    ///
    /// Values describe the downstream client connection and are immutable
    /// for the lifetime of the request.
    pub fn client(&self) -> &ClientMetadata {
        &self.metadata
    }

    /// Perform a subrequest to an absolute URL (SPEC §7).
    ///
    /// The URI of `req` must be absolute; adapters resolve its host to a
    /// backend. Fails with [`FetchError::BadRequest`](crate::FetchError::BadRequest)
    /// if the URI is relative. The response body is fully buffered (v1).
    pub async fn fetch(&mut self, req: EdgeRequest) -> Result<EdgeResponse> {
        self.platform.fetch(req).await
    }

    /// Perform a subrequest, returning its body as a stream (SPEC D21).
    ///
    /// Like [`Context::fetch`], but the response body is a
    /// [`crate::Body::Streaming`] source instead of buffered bytes: headers
    /// arrive first, and the body can be read incrementally with
    /// [`crate::Body::next_chunk`], relayed, or drained with
    /// [`crate::Body::collect`].
    pub async fn fetch_streaming(&mut self, req: EdgeRequest) -> Result<EdgeResponse> {
        self.platform.fetch_streaming(req).await
    }

    /// Look up a configuration variable, or `None` if unset.
    pub fn var(&self, name: &str) -> Option<String> {
        self.platform.var(name)
    }

    /// Look up a secret, or `None` if unset.
    pub fn secret(&self, name: &str) -> Option<Vec<u8>> {
        self.platform.secret(name)
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
        self.platform.kv(name)
    }

    /// Emit a log message. Prefer the `edge_core::log::{info,warn,error}!`
    /// macros.
    pub fn log(&self, level: LogLevel, message: &str) {
        self.platform.log(level, message);
    }

    /// Set an invocation-scoped structured log field
    /// (SPEC-PORTABILITY-PRIMITIVES §6, M11).
    ///
    /// Keys are normalized to lowercase ASCII and validated against
    /// `[a-z0-9][a-z0-9._-]*`; invalid keys return
    /// [`Error::LogField`](crate::Error::LogField). Empty values are
    /// omitted. Per-value and aggregate byte budgets are enforced with
    /// deterministic truncation and a diagnostic.
    pub fn set_log_field(&self, key: impl Into<String>, value: impl Into<String>) -> Result<()> {
        self.platform.set_log_field(key.into(), value.into())
    }

    /// Remove a structured log field (a no-op if absent).
    pub fn remove_log_field(&self, key: &str) {
        self.platform.remove_log_field(key);
    }

    /// Finalize structured log fields at the adapter boundary (adapter SPI;
    /// `#[doc(hidden)]`).
    ///
    /// Adapters call this once the handler completes, before the response is
    /// sent, so fields are captured for every outcome.
    #[doc(hidden)]
    pub fn finalize_log_fields(&self) -> Option<String> {
        self.platform.finalize_log_fields()
    }
}
