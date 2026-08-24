//! Cloudflare KV backend (SPEC §6.5).
//!
//! Parity decisions: `get` of a missing key is `Ok(None)` and `delete` of a
//! missing key succeeds — both native Cloudflare semantics (D5.3).
//! Note: values written by this adapter (ArrayBuffer) read back via
//! `bytes()`; text values written outside the adapter may read back as
//! `None` (KV value-type tagging).

use std::fmt;

use bytes::Bytes;
use edge_core::{
    error::{Error, KvError},
    kv::{KvBackend, KvValue},
    Body, Result,
};
use futures_util::future::BoxFuture;

use crate::send::SendFuture;

fn kv_err(e: worker::KvError) -> Error {
    Error::Kv(KvError::Platform(e.to_string()))
}

/// A `KvBackend` over a workers-rs [`worker::KvStore`].
pub struct CloudflareKvBackend {
    store: worker::KvStore,
}

impl CloudflareKvBackend {
    /// Wrap a namespace handle.
    pub fn new(store: worker::KvStore) -> Self {
        Self { store }
    }
}

impl fmt::Debug for CloudflareKvBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CloudflareKvBackend")
            .finish_non_exhaustive()
    }
}

impl KvBackend for CloudflareKvBackend {
    fn get(&self, key: &str) -> BoxFuture<'_, Result<Option<KvValue>>> {
        let store = self.store.clone();
        let key = key.to_string();
        Box::pin(SendFuture(async move {
            let bytes = store.get(&key).bytes().await.map_err(kv_err)?;
            Ok(bytes.map(|b| KvValue::from_body(Bytes::from(b))))
        }))
    }

    fn put(&self, key: &str, value: Body) -> BoxFuture<'_, Result<()>> {
        let store = self.store.clone();
        let key = key.to_string();
        Box::pin(SendFuture(async move {
            store
                .put_bytes(&key, value.as_ref())
                .map_err(kv_err)?
                .execute()
                .await
                .map_err(kv_err)
        }))
    }

    fn delete(&self, key: &str) -> BoxFuture<'_, Result<()>> {
        let store = self.store.clone();
        let key = key.to_string();
        Box::pin(SendFuture(async move {
            store.delete(&key).await.map_err(kv_err)
        }))
    }
}
