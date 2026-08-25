# KV (§6.5)

Split from `SPEC.md` §6.5 (2026-08-25). Part of the [API surface](README.md).

```rust
pub struct KvStore { /* opaque */ }

impl KvStore {
    pub async fn get(&self, key: &str) -> Result<Option<KvValue>>;
    pub async fn put(&self, key: &str, value: impl Into<Body>) -> Result<()>;
    pub async fn delete(&self, key: &str) -> Result<()>;
}

pub struct KvValue(/* opaque */);
impl KvValue {
    pub async fn text(self) -> Result<Option<String>>;   // None if invalid UTF-8? → use bytes
    pub async fn bytes(self) -> Result<Body>;
    pub async fn json<T: DeserializeOwned>(self) -> Result<Option<T>>;
}
```

Normalized semantics (parity decisions):
- `get` returns `Option`; not-found is `None` on both platforms.
- `put`/`delete` are fire-and-forget-ish; errors surface as `KvError`.
- TTL, metadata, and listing are **out of scope** in v1 (present on CF only).
- Documented limits: CF KV max value 25 MiB; Fastly KV value limit is smaller — adapters MUST document the effective limit; `put` MAY reject oversize values locally.
- KV values are bytes, not streams: `KvStore::put` drains streaming bodies to `Bytes` before reaching a backend (D21 — [decisions/d21](../decisions/d21.md)).

## See also

- [context](context.md) — `Context::kv()` / `kv_named()`
- [http-types](http-types.md) — `Body` values in `put`
