# Conformance suite (§11)

A shared suite compiled natively (mock context), under Viceroy, and under
workerd. Every test MUST pass identically on all three targets. Split from
`SPEC.md` §11 (2026-08-25). The suite lives in `tests/conformance/`
(handlers in `src/lib.rs`, drivers in `run.sh` / `run-cf.sh`, native tests
in `tests/native.rs`).

| # | Test | Parity requirement |
|---|---|---|
| T1 | Echo: method, path, query, headers, body round-trip | identical |
| T2 | Status + header passthrough, UTF-8 body | identical |
| T3 | Router: path params, 404, query extraction | identical |
| T4 | fetch to declared origin; origin echoes received Host header | Host == URL host on both |
| T5 | fetch to undeclared host | fail closed on Fastly; documented behavior on CF |
| T6 | fetch error surface (timeout/refused) → FetchError category | same category |
| T7 | vars/secrets for configured keys; None otherwise | identical |
| T8 | KV put/get/delete round trip; get missing → None | identical |
| T9 | logging macro emits to configured sink | message present |
| T10 | 1 MiB body buffering | identical |
| T11 | sequential fetch (two fetches, awaited in sequence) | works on both |
| T12 | streaming fetch + relay (M6): `fetch_streaming` reads one chunk, relays the remainder as a stream | invariant: first-chunk + relayed body == origin payload (chunk boundaries are platform-dependent) |
| P7 | client metadata fixture (M10) | available fields map; unavailable fields are `None` — on all three targets, driven by the mock fixture / Viceroy geolocation fixture / workerd `cf-blob` |
| P8 | original header list unavailable (M10) | `None`, never reconstructed — CF (no API) and mock; Fastly reports the original-header API result |
| P9 | log fields on success and synthetic error (M11) | same logical map captured on both — CF control header on the 200 and the 500; Fastly records in the log endpoint |
| P10 | origin injects logging control field (M11) | value stripped and diagnostic emitted — CF header carries only facade fields; Fastly record + no client-visible header |
| P11 | field budget exceeded (M11) | deterministic retained set (13 newest of 20×303B) — CF header / Fastly record contain exactly the retained set |

Portability conformance P1–P6, P12–P15 remain gated on M7–M9, M12–M14 —
see `portability/conformance` (planned).

## See also

- [milestones/README](milestones/README.md) — which milestones each test gates
- `portability/conformance` (planned) — the P1–P15 scenarios
