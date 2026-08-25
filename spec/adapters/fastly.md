# edge-fastly adapter (§8.2)

Split from `SPEC.md` §8.2 (2026-08-25). Part of the [adapter contracts](README.md).

Responsibilities:
- Convert `fastly::Request` ⇄ `EdgeRequest` via the `From`/`Into` `http` impls, then buffer the body (`read_to_end`).
- Build `Context::fastly()`:
  - `var`/`secret` from `ConfigStore`/`SecretStore` (names from embedded config).
  - `kv()` from the configured `KVStore`.
  - `fetch`: resolution chain [fetch §7.3](../fetch.md); `send` (sync) returns after response headers; buffered ([D2](../decisions/d02.md)). `fetch_streaming` ([D21](../decisions/d21.md)) keeps the body handle live as a `ChunkStream`.
  - `log` → configured log endpoint (`fastly::log`), fallback `eprintln!`.
- **Drive async from the sync entry** ([execution](execution.md)).
- Map errors per [api/errors](../api/errors.md).

Implemented additionally in M10/M11: client metadata from the downstream
client IP/POP/original-header/geo APIs ([portability/client-metadata](../portability/client-metadata.md)),
one structured log record per request to the log endpoint
([portability/log-fields](../portability/log-fields.md)).

## See also

- [execution](execution.md) — the executor contract (§8.3)
- [capability-matrix](../capability-matrix.md) — Fastly SDK facts
- [D14](../decisions/d14.md) — no host-run tests (link constraint)
