//! KV store abstraction (SPEC §6.5).

use std::fmt;
use std::sync::Arc;

use futures_util::future::BoxFuture;
use serde::de::DeserializeOwned;

use crate::error::{Error, KvError};
use crate::types::Body;
use crate::Result;

/// Internal KV backend SPI, implemented by platform adapters and the mock.
///
/// **Not part of the stable public API** (`#[doc(hidden)]`).
#[doc(hidden)]
pub trait KvBackend: Send + Sync + fmt::Debug {
    /// Fetch the value for `key`, or `None` if absent.
    fn get(&self, key: &str) -> BoxFuture<'_, Result<Option<KvValue>>>;

    /// Store `value` under `key`.
    fn put(&self, key: &str, value: Body) -> BoxFuture<'_, Result<()>>;

    /// Remove `key`.
    fn delete(&self, key: &str) -> BoxFuture<'_, Result<()>>;
}

/// A handle to a named KV store.
///
/// Cheap to clone; all operations are `async`.
#[derive(Clone)]
pub struct KvStore(Arc<dyn KvBackend>);

impl fmt::Debug for KvStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("KvStore { .. }")
    }
}

impl KvStore {
    /// Wrap a backend implementation (adapter SPI; `#[doc(hidden)]`).
    #[doc(hidden)]
    pub fn from_backend(backend: Box<dyn KvBackend>) -> Self {
        Self(Arc::from(backend))
    }

    /// Fetch the value for `key`.
    ///
    /// Returns `None` when the key does not exist (SPEC D5.3).
    pub async fn get(&self, key: &str) -> Result<Option<KvValue>> {
        self.0.get(key).await
    }

    /// Store `value` under `key`, overwriting any existing value.
    pub async fn put(&self, key: &str, value: impl Into<Body>) -> Result<()> {
        self.0.put(key, value.into()).await
    }

    /// Remove `key`. Succeeds whether or not the key existed.
    pub async fn delete(&self, key: &str) -> Result<()> {
        self.0.delete(key).await
    }
}

/// A KV value, decoded on demand.
#[derive(Debug, Clone)]
pub struct KvValue(Body);

impl KvValue {
    /// Wrap a raw body (adapter SPI; `#[doc(hidden)]`).
    #[doc(hidden)]
    pub fn from_body(body: Body) -> Self {
        Self(body)
    }

    /// The raw bytes.
    pub async fn bytes(self) -> Result<Body> {
        Ok(self.0)
    }

    /// Decode as UTF-8 text.
    ///
    /// Returns `Ok(None)` if the value is not valid UTF-8 (SPEC §6.5).
    pub async fn text(self) -> Result<Option<String>> {
        match String::from_utf8(self.0.to_vec()) {
            Ok(s) => Ok(Some(s)),
            Err(_) => Ok(None),
        }
    }

    /// Deserialize as JSON.
    ///
    /// Fails with [`KvError`] if the value is not valid JSON for `T`.
    pub async fn json<T: DeserializeOwned>(self) -> Result<Option<T>> {
        serde_json::from_slice(&self.0)
            .map(Some)
            .map_err(|e| Error::Kv(KvError::Platform(format!("invalid JSON value: {e}"))))
    }
}
