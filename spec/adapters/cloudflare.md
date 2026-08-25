# edge-cloudflare adapter (§8.1)

Split from `SPEC.md` §8.1 (2026-08-25). Part of the [adapter contracts](README.md).

Responsibilities:
- Convert `web_sys::Request` ⇄ `EdgeRequest` (method, URI, headers, buffered body) — prefer the `worker` crate `http`-feature helpers (`request_from_wasm`, `response_to_wasm`, …) and normalize bodies via `Body::bytes()`.
- Build `Context::cloudflare(env)`:
  - `var`/`secret` from `Env` bindings.
  - `kv()` from the configured namespace binding (name from embedded config).
  - `fetch`: build `web_sys::Request` from `EdgeRequest`, call `Fetcher::fetch_request`/`Fetch::run` with `redirect: manual`; map errors per [api/errors](../api/errors.md).
  - `log` → `console_log!`/`console_warn!`/`console_error!`.
- Drive the handler on the JS event loop (async native — no executor needed).
- Never hold `!Send` JS objects across the handler boundary: everything crossing into `edge-core` is plain data (`Bytes`, `String`).

Implemented additionally in M2/M10/M11: empty-body normalization ([D17](../decisions/d17.md)), `Send` bridging ([D18](../decisions/d18.md)), client metadata from `cf-connecting-ip` + `request.cf` (`portability/client-metadata` (planned)), log-field control header (`portability/log-fields` (planned)).

## See also

- [execution](execution.md) — the async model (§8.3)
- [capability-matrix](../capability-matrix.md) — CF SDK facts
