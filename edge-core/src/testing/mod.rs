//! Native mock platform for tests and the conformance harness.
//!
//! `MockContextBuilder` configures vars, secrets, KV entries, a fetch handler
//! and fault injection; `MockContext` records all platform interactions for
//! assertions.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use futures_util::future::BoxFuture;

use crate::context::{Context, LogLevel, Platform};
use crate::error::{Error, FetchError, KvError};
use crate::kv::{KvBackend, KvStore, KvValue};
use crate::types::{Body, EdgeRequest, EdgeResponse};
use crate::Result;

/// Fault injection switches.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MockFaults {
    /// Every `fetch` fails with [`FetchError::Connection`].
    pub fail_fetch: bool,
    /// Every KV operation fails with [`KvError::Platform`].
    pub fail_kv: bool,
}

/// A record of platform interactions, for assertions.
#[derive(Debug, Clone, Default)]
pub struct Records {
    /// Log messages in emission order: `(level, message)`.
    pub logs: Vec<(LogLevel, String)>,
    /// Requests passed to `fetch`, in order.
    pub fetches: Vec<EdgeRequest>,
    /// KV operations in order, formatted `"{op}:{store}:{key}"`.
    pub kv_ops: Vec<String>,
}

/// The mock platform implementation.
#[derive(Default)]
pub(crate) struct MockPlatform {
    vars: HashMap<String, String>,
    secrets: HashMap<String, Vec<u8>>,
    kv_stores: HashMap<String, Arc<Mutex<HashMap<String, Bytes>>>>,
    fetch_handler: Option<Arc<dyn Fn(EdgeRequest) -> Result<EdgeResponse> + Send + Sync>>,
    faults: MockFaults,
    records: Arc<Mutex<Records>>,
}

impl fmt::Debug for MockPlatform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MockPlatform")
            .field("vars", &self.vars)
            .field("secrets", &self.secrets.len())
            .field("kv_stores", &self.kv_stores.len())
            .field("faults", &self.faults)
            .finish_non_exhaustive()
    }
}

impl Platform for MockPlatform {
    fn fetch(&self, req: EdgeRequest) -> BoxFuture<'_, Result<EdgeResponse>> {
        let records = Arc::clone(&self.records);
        let handler = self.fetch_handler.clone();
        let faults = self.faults;
        let host = req.uri().host().map(str::to_owned).unwrap_or_default();
        Box::pin(async move {
            records
                .lock()
                .expect("mock records poisoned")
                .fetches
                .push(req.clone());
            if faults.fail_fetch {
                return Err(Error::Fetch(FetchError::Connection(
                    "mock fetch disabled".to_string(),
                )));
            }
            match handler {
                Some(f) => f(req),
                None => Err(Error::Fetch(FetchError::UnresolvedBackend(host))),
            }
        })
    }

    fn var(&self, name: &str) -> Option<String> {
        self.vars.get(name).cloned()
    }

    fn secret(&self, name: &str) -> Option<Vec<u8>> {
        self.secrets.get(name).cloned()
    }

    fn kv(&self, name: &str) -> Result<KvStore> {
        let data = self
            .kv_stores
            .get(name)
            .ok_or_else(|| Error::Kv(KvError::Platform(format!("no KV store named `{name}`"))))?;
        let backend = MockKvBackend {
            store: name.to_string(),
            data: Arc::clone(data),
            faults: self.faults,
            records: Arc::clone(&self.records),
        };
        Ok(KvStore::from_backend(Box::new(backend)))
    }

    fn log(&self, level: LogLevel, message: &str) {
        self.records
            .lock()
            .expect("mock records poisoned")
            .logs
            .push((level, message.to_string()));
    }
}

/// KV backend backed by an in-memory map.
struct MockKvBackend {
    store: String,
    data: Arc<Mutex<HashMap<String, Bytes>>>,
    faults: MockFaults,
    records: Arc<Mutex<Records>>,
}

impl fmt::Debug for MockKvBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MockKvBackend")
            .field("store", &self.store)
            .field("faults", &self.faults)
            .finish_non_exhaustive()
    }
}

impl MockKvBackend {
    fn fail(&self) -> bool {
        self.faults.fail_kv
    }
}

impl KvBackend for MockKvBackend {
    fn get(&self, key: &str) -> BoxFuture<'_, Result<Option<KvValue>>> {
        let key = key.to_string();
        let data = Arc::clone(&self.data);
        let records = Arc::clone(&self.records);
        let store = self.store.clone();
        let fail = self.fail();
        Box::pin(async move {
            if fail {
                return Err(Error::Kv(KvError::Platform("mock kv disabled".to_string())));
            }
            records
                .lock()
                .expect("mock records poisoned")
                .kv_ops
                .push(format!("get:{store}:{key}"));
            let value = data.lock().expect("mock kv poisoned").get(&key).cloned();
            Ok(value.map(KvValue::from_body))
        })
    }

    fn put(&self, key: &str, value: Body) -> BoxFuture<'_, Result<()>> {
        let key = key.to_string();
        let data = Arc::clone(&self.data);
        let records = Arc::clone(&self.records);
        let store = self.store.clone();
        let fail = self.fail();
        Box::pin(async move {
            if fail {
                return Err(Error::Kv(KvError::Platform("mock kv disabled".to_string())));
            }
            records
                .lock()
                .expect("mock records poisoned")
                .kv_ops
                .push(format!("put:{store}:{key}"));
            data.lock().expect("mock kv poisoned").insert(key, value);
            Ok(())
        })
    }

    fn delete(&self, key: &str) -> BoxFuture<'_, Result<()>> {
        let key = key.to_string();
        let data = Arc::clone(&self.data);
        let records = Arc::clone(&self.records);
        let store = self.store.clone();
        let fail = self.fail();
        Box::pin(async move {
            if fail {
                return Err(Error::Kv(KvError::Platform("mock kv disabled".to_string())));
            }
            records
                .lock()
                .expect("mock records poisoned")
                .kv_ops
                .push(format!("delete:{store}:{key}"));
            data.lock().expect("mock kv poisoned").remove(&key);
            Ok(())
        })
    }
}

/// Builder for a mock platform. Configure, then call [`build`](Self::build).
#[derive(Default)]
pub struct MockContextBuilder {
    vars: HashMap<String, String>,
    secrets: HashMap<String, Vec<u8>>,
    kv_stores: HashMap<String, Arc<Mutex<HashMap<String, Bytes>>>>,
    fetch_handler: Option<Arc<dyn Fn(EdgeRequest) -> Result<EdgeResponse> + Send + Sync>>,
    faults: MockFaults,
}

impl fmt::Debug for MockContextBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MockContextBuilder")
            .field("vars", &self.vars)
            .field("secrets", &self.secrets.len())
            .field("kv_stores", &self.kv_stores.len())
            .field("faults", &self.faults)
            .finish_non_exhaustive()
    }
}

impl MockContextBuilder {
    /// Create an empty builder. The default KV store (`"default"`) always
    /// exists on the built context.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set a configuration variable.
    pub fn var(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.vars.insert(name.into(), value.into());
        self
    }

    /// Set a secret.
    pub fn secret(mut self, name: impl Into<String>, value: impl Into<Vec<u8>>) -> Self {
        self.secrets.insert(name.into(), value.into());
        self
    }

    /// Seed a KV entry in the given store (`"default"` unless overridden).
    pub fn kv_entry(
        mut self,
        store: impl Into<String>,
        key: impl Into<String>,
        value: impl Into<Body>,
    ) -> Self {
        self.kv_stores
            .entry(store.into())
            .or_default()
            .lock()
            .expect("mock kv poisoned")
            .insert(key.into(), value.into());
        self
    }

    /// Install the fetch handler. The closure receives the request (like a
    /// local origin) and returns the response.
    pub fn on_fetch(
        mut self,
        handler: impl Fn(EdgeRequest) -> Result<EdgeResponse> + Send + Sync + 'static,
    ) -> Self {
        self.fetch_handler = Some(Arc::new(handler));
        self
    }

    /// Make every `fetch` fail with [`FetchError::Connection`].
    pub fn fail_fetch(mut self) -> Self {
        self.faults.fail_fetch = true;
        self
    }

    /// Make every KV operation fail with [`KvError::Platform`].
    pub fn fail_kv(mut self) -> Self {
        self.faults.fail_kv = true;
        self
    }

    /// Build the mock context.
    pub fn build(self) -> MockContext {
        let records = Arc::new(Mutex::new(Records::default()));
        let mut kv_stores = self.kv_stores;
        kv_stores.entry("default".to_string()).or_default();
        let platform = MockPlatform {
            vars: self.vars,
            secrets: self.secrets,
            kv_stores,
            fetch_handler: self.fetch_handler,
            faults: self.faults,
            records: Arc::clone(&records),
        };
        MockContext {
            ctx: Context::from_platform(Box::new(platform)),
            records,
        }
    }
}

/// A built mock context plus its interaction records.
#[derive(Clone)]
pub struct MockContext {
    ctx: Context,
    records: Arc<Mutex<Records>>,
}

impl fmt::Debug for MockContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MockContext")
            .field("records", &self.records())
            .finish()
    }
}

impl MockContext {
    /// A cloned [`Context`] to pass into handlers/routers.
    pub fn context(&self) -> Context {
        self.ctx.clone()
    }

    /// Snapshot of recorded platform interactions.
    pub fn records(&self) -> Records {
        self.records.lock().expect("mock records poisoned").clone()
    }
}
