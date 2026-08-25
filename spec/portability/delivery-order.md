# Delivery order (§12)

Split from `SPEC-PORTABILITY-PRIMITIVES.md` §12 (2026-08-25). Part of the
[portability primitives](README.md) (draft v0.2). Original section number
preserved as a heading so references ("§12") keep resolving.

Milestone ownership: see `SPEC.md` §12, M7–M14.

1. Wake-capable Fastly executor, monotonic clock, and deadlines.
2. Fetch timeout and client-disconnect behavior.
3. Deferred work.
4. Client metadata. **Shipped (M10, 2026-08-25)** — P7/P8 green on host + Viceroy + workerd.
5. Structured logging fields. **Shipped (M11, 2026-08-25)** — P9/P10/P11 green on host + Viceroy + workerd.
6. Rate-limit adapters and code generation.
7. Optional scheduled delivery.

KV-backed dictionaries require no new facade primitive beyond the existing KV API. Caching and prewarming can be added independently after measurement.
