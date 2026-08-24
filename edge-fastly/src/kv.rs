//! Fastly KV backend (SPEC §6.5).
//!
//! Parity decisions: missing keys are `Ok(None)` on `get` and `Ok(())` on
//! `delete` (D5.3 / SPEC §6.5). Other host failures surface as
//! [`KvError::Platform`].

use std::fmt;

use bytes::Bytes;
use edge_core::{
    error::{Error, KvError},
    kv::{KvBackend, KvStore, KvValue},
    Result,
};
use fastly::kv_store::{KVStore, KVStoreError};
use futures_util::future::BoxFuture;

/// A `KvBackend` over a Fastly [`KVStore`].
pub struct FastlyKvBackend {
    store: KVStore,
}

impl FastlyKvBackend {
    /// Wrap an opened store.
    pub fn new(store: KVStore) -> Self {
        Self { store }
    }
}

impl fmt::Debug for FastlyKvBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FastlyKvBackend").finish_non_exhaustive()
    }
}

impl KvBackend for FastlyKvBackend {
    fn get(&self, key: &str) -> BoxFuture<'_, Result<Option<KvValue>>> {
        // All blocking host calls; resolves on the first poll (SPEC §8.3).
        let result = match self.store.lookup(key) {
            Ok(mut resp) => Ok(Some(KvValue::from_bytes(Bytes::from(
                resp.take_body_bytes(),
            )))),
            // D5.3: not-found is None, not an error.
            Err(KVStoreError::ItemNotFound) => Ok(None),
            Err(e) => Err(Error::Kv(KvError::Platform(e.to_string()))),
        };
        Box::pin(async move { result })
    }

    fn put(&self, key: &str, value: Bytes) -> BoxFuture<'_, Result<()>> {
        let result = self
            .store
            .insert(key, fastly::Body::from(value.to_vec()))
            .map_err(|e| Error::Kv(KvError::Platform(e.to_string())));
        Box::pin(async move { result })
    }

    fn delete(&self, key: &str) -> BoxFuture<'_, Result<()>> {
        // SPEC §6.5: succeeds whether or not the key existed.
        let result = match self.store.delete(key) {
            Ok(()) => Ok(()),
            Err(KVStoreError::ItemNotFound) => Ok(()),
            Err(e) => Err(Error::Kv(KvError::Platform(e.to_string()))),
        };
        Box::pin(async move { result })
    }
}

/// Convenience for constructing a store handle inside [`crate::platform`].
pub(crate) fn wrap(store: KVStore) -> KvStore {
    KvStore::from_backend(Box::new(FastlyKvBackend::new(store)))
}
