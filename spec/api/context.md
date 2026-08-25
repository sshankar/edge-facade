# Context (§6.3)

Split from `SPEC.md` §6.3 (2026-08-25). Part of the [API surface](README.md).

```rust
pub struct Context { /* opaque; constructed only by adapters */ }

impl Context {
    // Subrequest (URL-based). See fetch.
    pub async fn fetch(&mut self, req: EdgeRequest) -> Result<EdgeResponse>;

    // Config
    pub fn var(&self, name: &str) -> Option<String>;
    pub fn secret(&self, name: &str) -> Option<Vec<u8>>;

    // KV — default configured store; named variant for multi-store services
    pub fn kv(&self) -> KvStore;
    pub fn kv_named(&self, name: &str) -> Result<KvStore>;

    // Logging
    pub fn log(&self, level: LogLevel, message: &str);
}

pub enum LogLevel { Info, Warn, Error }
```

Logging convenience macros in `edge-core::log`:

```rust
edge::log::info!(...);   // → console_log! on CF; fastly log endpoint on Fastly
edge::log::error!(...);
```

On Fastly, the log destination is the endpoint named by `[logging] endpoint` in shared config, falling back to `eprintln!` (captured by Viceroy).

**Extended in M10/M11:** `Context::client()` returns the client metadata snapshot ([portability/client-metadata](../portability/client-metadata.md)); `Context::set_log_field`/`remove_log_field` manage structured log fields ([portability/log-fields](../portability/log-fields.md)).

## See also

- [handler](handler.md) — how the handler receives `Context`
- [kv](kv.md) — `KvStore` (§6.5)
- [fetch](../fetch.md) — the subrequest API (§7)
