# Error model (§6.4)

Split from `SPEC.md` §6.4 (2026-08-25). Part of the [API surface](README.md).

```rust
pub enum Error {
    // Common, semantics-normalized
    Fetch(FetchError),
    Kv(KvError),
    Config(String),
    Router(PathError),
    Body(std::io::Error),           // buffering failures
    LogField(String),               // structured log field validation (M11)
    Internal(String),               // panics/unknown — 500 path
}

pub enum FetchError {
    UnresolvedBackend(String),      // host has no backend and dynamic fallback disabled
    Connection(String),             // connect/DNS/refused
    Tls(String),
    Timeout,
    Permission,                     // CF fetch binding/allowlist; Fastly Disallowed
    BadRequest(String),             // malformed URL / request
    Platform(String),               // anything else, with platform prefix
}
```

Mappings (MUST be lossless at the category level):
- CF: `TypeError`/fetch failures → `Connection | Tls | Timeout | Permission` (in practice all map to `Connection` — [D16](../decisions/d16.md)).
- Fastly: `SendError`/`SendErrorCause` → `Connection | Tls | Timeout`; `BackendCreationError::Disallowed` → `Permission`; missing backend → `UnresolvedBackend`.
- The error type implements `std::error::Error`, `Display`, and `Into<worker::Error>` / `Into<fastly::Error>` for adapter boundaries.

## See also

- [D16](../decisions/d16.md) — CF fetch errors map to `Connection`
- [D12](../decisions/d12.md) — adapter-level error handling (404 for router misses)
