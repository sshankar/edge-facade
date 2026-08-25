# Open questions & risks (§14)

Split from `SPEC.md` §14 (2026-08-25).

1. **CF fetch permission syntax:** resolved 2026-08-24 — verified against wrangler 4.125.0's `config-schema.json`: no `permissions`/fetch-allowlist key exists (only generic `unsafe.bindings`). Outbound `fetch` is allow-by-default on Workers; `edge-cli` emits no permission config. The fail-closed Fastly asymmetry ([fetch §7.5](fetch.md)) remains documented, not configurable, on CF.
2. **Fastly Host/SNI semantics:** exact behavior of `send()` w.r.t. `Host` header when `override_host` unset — must be empirically confirmed in M3 (Viceroy echo origin). *(Resolved in M3 via the echo-origin test — [fetch §7.4](fetch.md).)*
3. **Dynamic backends:** per-service enablement; creation cost per session; whether `enable_pooling`/`max_use` should be surfaced in config.
4. **KV limits:** exact Fastly KV value size limit; decide local reject vs pass-through.
5. **MSRV drift:** workers-rs 1.75 vs fastly 1.88 → SDK MSRV 1.88; revisit if fastly raises.
6. **Send/Sync policy:** v1 requires plain-data boundaries so core types are `Send`/`Sync`; handler futures remain non-`Send` (matches both platforms' single-threaded reality; axum reuse later would require revisiting — [D7](decisions/d07.md), [D18](decisions/d18.md)).
7. **Package naming:** `edge` may collide on crates.io; final names TBD (`edge-kit`, `duo-edge`, …).
8. **Redirect parity:** choosing "manual on CF" loses CF's convenience; documented tradeoff ([D5](decisions/d05.md)), revisit in v2 with an opt-in `follow_redirects` flag implemented in core.
