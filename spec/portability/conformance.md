# Conformance (§11)

Split from `SPEC-PORTABILITY-PRIMITIVES.md` §11 (2026-08-25). Part of the
[portability primitives](README.md) (draft v0.2). Original section number
preserved as a heading so references ("§11") keep resolving.

| # | Scenario | Required result |
|---|---|---|
| P1 | register deferred work then return | response completes first; work is subsequently observed |
| P2 | deferred work fails | response unchanged; diagnostic emitted; later work runs |
| P3 | client disconnect during awaited fetch | origin operation is not cancelled solely by disconnect |
| P4 | fetch exceeds deadline | `TimeoutScope::Fetch` |
| P5 | pipeline exceeds request deadline at an await point | `TimeoutScope::Request` |
| P6 | nested deadlines | earliest deadline wins |
| P7 | client metadata fixture | available fields map; unavailable fields are `None` |
| P8 | original header list unavailable | `None`, never reconstructed |
| P9 | fields on success and synthetic error | same logical map is captured |
| P10 | origin injects logging control field | value stripped and diagnostic emitted |
| P11 | field budget exceeded | deterministic retained set; no client-visible control data |
| P12 | JSON dictionary lookup through KV | same `get` and `get_all` results |
| P13 | rate limit below/above threshold | allowed then denied according to policy |
| P14 | rate-limit timeout in open/closed modes | allowed/denied with timeout failure |
| P15 | scheduled maintenance invoked twice | idempotent under at-least-once delivery |

P1-P14 run natively and under workerd/Viceroy. P15 runs under workerd and through the configured Fastly delivery harness.
