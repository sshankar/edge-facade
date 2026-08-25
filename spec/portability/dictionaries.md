# KV-backed dictionaries (§7)

Split from `SPEC-PORTABILITY-PRIMITIVES.md` §7 (2026-08-25). Part of the
[portability primitives](README.md) (draft v0.2). Original section number
preserved as a heading so references ("§7") keep resolving.

The common KV API is sufficient to build named, read-mostly dictionaries. No dictionary cache is required in `edge-facade`.

A portable application library may store one dictionary as a JSON object under one KV key:

```rust
pub struct DictionaryStore {
    kv: KvStore,
}

impl DictionaryStore {
    pub async fn get(&self, dictionary: &str, key: &str) -> Result<Option<String>>;
    pub async fn get_all(&self, dictionary: &str) -> Result<Map<String, String>>;
}
```

Storage and caching rules:

- Cloudflare KV and Fastly KV are the backing stores through `Context::kv()`.
- The simplest implementation fetches and parses the JSON value on demand.
- An application library MAY cache the parsed value in instance memory when repeated lookups justify it.
- Provider-specific cache layers, stale-while-revalidate, in-flight deduplication, and prewarming are optional optimizations.
- Applications that control the data model MAY store entries as individual KV keys instead; this changes bulk-read and consistency behavior and is not imposed by the facade.
- Read-only configuration may use a provider config store behind a separate abstraction, but dynamic dictionaries use KV.
- Scheduled prewarming is unnecessary for correctness. It is added only when latency measurements justify it.

This keeps lookup semantics portable while allowing each provider's KV implementation to supply its own internal caching.
