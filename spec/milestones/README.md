# Milestone roadmap (§12)

One page per milestone ([m0](m0.md) … [m14](m14.md)) with deliverable, exit
criteria, status, and links to the implementation plan, decisions, and
conformance scenarios. Split from `SPEC.md` §12 (2026-08-25).

M0–M6 deliver the v1 core. M7+ deliver the runtime portability primitives
of the draft v0.2 extension in its delivery order
(`portability/delivery-order` (planned)): the
wake-capable Fastly executor and monotonic clock come first — they unblock
timeouts and deferred work on Fastly — then fetch behavior, deferred work,
client metadata, structured log fields, rate limiting, and optional
scheduled delivery. P1–P15 are the portability conformance scenarios
(`portability/conformance` (planned)); each M7+ row's
exit criteria name the P-tests that gate it.

| M | Title | Status |
|---|---|---|
| [M0](m0.md) | edge-core + native mock context | ✅ done (2026-08-21) |
| [M1](m1.md) | edge-fastly adapter + edge-macros | ✅ done (2026-08-21); live Fastly pending account |
| [M2](m2.md) | edge-cloudflare adapter | ✅ done (2026-08-22); live CF pending account |
| [M3](m3.md) | fetch resolver + parity rules | ✅ done (2026-08-24) |
| [M4](m4.md) | config vars/secrets + KV | ✅ done (2026-08-24) |
| [M5](m5.md) | router, logging, edge-cli, conformance CI, docs | ✅ done (2026-08-24) |
| [M6](m6.md) | streaming response bodies | ✅ done (2026-08-24) |
| [M7](m7.md) | wake-capable Fastly executor + deadlines | — pending |
| [M8](m8.md) | fetch options | — pending |
| [M9](m9.md) | deferred work | — pending |
| [M10](m10.md) | client metadata | ✅ done (2026-08-25) |
| [M11](m11.md) | structured logging fields | ✅ done (2026-08-25) |
| [M12](m12.md) | rate limiting | — pending |
| [M13](m13.md) | scheduled events (optional) | — pending |
| [M14](m14.md) | KV-backed dictionaries (optional) | — pending |

## See also

- [conformance](../conformance.md) — T1–T12 + P7–P11 scenarios
- `portability/delivery-order` (planned) — the M7+ delivery order
