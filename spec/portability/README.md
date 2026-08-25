# Runtime portability primitives (draft v0.2)

Common runtime capabilities needed by production edge applications on
Cloudflare Workers and Fastly Compute. Split from
`SPEC-PORTABILITY-PRIMITIVES.md` (2026-08-25). This document extends the
[spec wiki](../README.md); if adopted, it supersedes the v1 exclusions for
scheduled events and client metadata only to the extent described in
[overview §2](../overview.md). Application frameworks and policies remain
outside `edge-facade`.

## Goals

The SDK MUST let shared Rust application code:

1. keep bounded work alive after the response path stops awaiting it;
2. enforce per-operation and whole-request wall-clock deadlines;
3. read normalized client, edge, geography, and TLS metadata;
4. record structured fields on every completed request without exposing them to the client;
5. invoke named edge rate-limit policies through either provider; and
6. run optional scheduled maintenance.

The SDK exposes capabilities and lifecycle semantics. Applications decide how to route requests, synthesize responses, construct headers, retry origins, cache data, and enforce policy.

## Non-goals

- Providing an application framework or request-processing pipeline.
- Defining application-specific headers, log schemas, or security policy.
- Implementing retries, block lists, authentication, CORS, cookie rewriting, or synthetic HTML.
- Implementing a mandatory dictionary cache, L1/L2 hierarchy, stale-while-revalidate policy, or prewarming strategy.
- Guaranteeing identical internal rate-counter values across vendors.
- Providing durable background jobs. Deferred work remains best effort and invocation-scoped.
- Inventing a provider-owned cron service where none exists.

## Pages

| Page | § | Topic | Status |
|---|---|---|---|
| [deferred-work](deferred-work.md) | 3 | `wait_until`, fetch options & client disconnects | draft |
| [time-deadlines](time-deadlines.md) | 4 | monotonic clock, `timeout`/`elapsed`/`remaining`, Fastly executor requirement | draft |
| [client-metadata](client-metadata.md) | 5 | `Context::client()` snapshot | ✅ shipped (M10, 2026-08-25) |
| [log-fields](log-fields.md) | 6 | `set_log_field`/`remove_log_field` | ✅ shipped (M11, 2026-08-25) |
| [dictionaries](dictionaries.md) | 7 | KV-backed `DictionaryStore` (application library) | draft |
| [scheduled](scheduled.md) | 8 | `#[edge::scheduled]` maintenance | draft |
| [rate-limiting](rate-limiting.md) | 9 | `RateLimiter`, policy config | draft |
| [config](config.md) | 10 | runtime/rate-limit/schedule config schema | draft |
| [conformance](conformance.md) | 11 | P1–P15 scenarios | P7–P11 green |
| [delivery-order](delivery-order.md) | 12 | M7+ delivery order | M10/M11 shipped |

Milestone ownership of these pages: [milestones/README](../milestones/README.md) M7–M14.
