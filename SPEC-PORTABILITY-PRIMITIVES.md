# Runtime Portability Primitives

**Status:** Draft v0.2  
**Scope:** Common runtime capabilities needed by production edge applications on Cloudflare Workers and Fastly Compute.

This document extends `SPEC.md`. If adopted, it supersedes the v1 exclusions for scheduled events and client metadata only to the extent described here. Application frameworks and policies remain outside `edge-facade`.

## 1. Goals

The SDK MUST let shared Rust application code:

1. keep bounded work alive after the response path stops awaiting it;
2. enforce per-operation and whole-request wall-clock deadlines;
3. read normalized client, edge, geography, and TLS metadata;
4. record structured fields on every completed request without exposing them to the client;
5. invoke named edge rate-limit policies through either provider; and
6. run optional scheduled maintenance.

The SDK exposes capabilities and lifecycle semantics. Applications decide how to route requests, synthesize responses, construct headers, retry origins, cache data, and enforce policy.

## 2. Non-goals

- Providing an application framework or request-processing pipeline.
- Defining application-specific headers, log schemas, or security policy.
- Implementing retries, block lists, authentication, CORS, cookie rewriting, or synthetic HTML.
- Implementing a mandatory dictionary cache, L1/L2 hierarchy, stale-while-revalidate policy, or prewarming strategy.
- Guaranteeing identical internal rate-counter values across vendors.
- Providing durable background jobs. Deferred work remains best effort and invocation-scoped.
- Inventing a provider-owned cron service where none exists.

## 3. Deferred work

```rust
use std::future::Future;

impl Context {
    pub fn wait_until<F>(&self, work: F) -> Result<()>
    where
        F: Future<Output = Result<()>> + Send + 'static;
}
```

`wait_until` registers invocation-scoped work and returns immediately.

Required behavior:

- Registered work MUST NOT delay creation of the response.
- The runtime SHOULD continue registered work after the response is committed, subject to platform limits.
- Work is best effort and may be stopped by process termination, deployment replacement, or runtime limits.
- A deferred error MUST NOT alter a completed response. The adapter MUST log it.
- Deferred futures MUST own their data and MUST NOT borrow request or response stack values.
- Registration after response finalization MUST fail.
- The adapter MUST bound task count and SHOULD bound aggregate execution time.
- Deferred work MUST NOT be stored globally or shared between invocations.
- Logging fields for the response are frozen when the handler returns.

Platform mapping:

- **Cloudflare:** register the future with the native execution context.
- **Fastly:** queue work in `Context`, commit the response, and then drain the queue while the invocation remains alive.
- **Native tests:** collect work in a deterministic queue exposed through `drain_deferred()`.

### 3.1 Fetch and client disconnects

Applications may need an awaited origin request to continue even if the downstream client disconnects. Registering and awaiting the same Rust future is not possible because a future is consumed once, so fetch owns this behavior:

```rust
pub struct FetchOptions {
    pub timeout: Option<Duration>,
    pub client_disconnect: ClientDisconnectPolicy,
}

pub enum ClientDisconnectPolicy {
    Ignore,
}

impl Context {
    pub async fn fetch_with(
        &self,
        req: EdgeRequest,
        options: FetchOptions,
    ) -> Result<EdgeResponse>;
}
```

- `Ignore` is the portable default and matches Fastly backend-fetch behavior.
- Cloudflare MUST decouple the outbound fetch from the inbound abort signal and retain the native fetch promise while it is awaited.
- A fetch deadline still cancels or abandons the outbound operation according to platform capability.
- `wait_until` remains available for cache writes, refreshes, metrics, and other work not awaited by the response path.

## 4. Time and deadlines

```rust
pub enum TimeoutScope {
    Request,
    Fetch,
    RateLimit,
    Application(&'static str),
}

pub struct TimeoutError {
    pub scope: TimeoutScope,
    pub limit: Duration,
}

impl Context {
    pub async fn timeout<F, T>(
        &self,
        scope: TimeoutScope,
        limit: Duration,
        future: F,
    ) -> std::result::Result<T, TimeoutError>
    where
        F: Future<Output = T> + Send;

    pub fn elapsed(&self) -> Duration;
    pub fn remaining(&self) -> Option<Duration>;
}
```

Required behavior:

- Time MUST use a monotonic clock.
- `timeout` races the future against a wall-clock timer and drops the future when the timer wins.
- Cancellation is cooperative; CPU-bound code that never yields cannot be preempted.
- Nested operations use the earliest active deadline.
- Adapter operations cap platform timeouts by `Context::remaining()`.
- `FetchOptions::timeout` is a per-attempt fetch timeout.
- A whole pipeline can be bounded with `TimeoutScope::Request`.
- Timeout errors remain distinguishable from connection, platform, and application errors.

Platform mapping:

- **Cloudflare:** use runtime timers and an adapter-owned abort signal for fetch deadlines.
- **Fastly:** use a monotonic clock and wake-capable executor. Backend connect, first-byte, and between-bytes limits are capped by the active deadline.
- **Native tests:** use an injectable clock.

### 4.1 Fastly executor requirement

The current `SPEC.md` D3 executor assumes every Fastly adapter future resolves on its first poll. Timers, timeout races, and deferred work violate that assumption.

This extension requires a Fastly executor that:

- parks rather than busy-spinning when no task is ready;
- wakes for monotonic timers;
- drives a handler and timer concurrently;
- commits the response before draining deferred work; and
- preserves sequential host-I/O compatibility where concurrent operations are unavailable.

Until this executor exists, general request timeouts and deferred work are not implemented on Fastly.

## 5. Client metadata

The adapter MUST snapshot request-entry metadata into owned, platform-free data:

```rust
pub struct ClientMetadata {
    pub provider: EdgeProvider,
    pub client_ip: Option<IpAddr>,
    pub pop: Option<String>,
    pub original_header_names: Option<Vec<String>>,
    pub geo: GeoMetadata,
    pub network: NetworkMetadata,
    pub tls: TlsMetadata,
}

pub enum EdgeProvider {
    Cloudflare,
    Fastly,
    Mock,
}

pub struct GeoMetadata {
    pub continent: Option<String>,
    pub country_code: Option<String>,
    pub region_code: Option<String>,
    pub city: Option<String>,
    pub postal_code: Option<String>,
    pub metro_code: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
}

pub struct NetworkMetadata {
    pub asn: Option<u32>,
    pub as_organization: Option<String>,
    pub proxy_type: Option<String>,
    pub proxy_description: Option<String>,
}

pub struct TlsMetadata {
    pub protocol: Option<String>,
    pub cipher: Option<String>,
    pub ja3: Option<String>,
    pub ja4: Option<String>,
    pub ciphers_sha1: Option<String>,
    pub extensions_sha1: Option<String>,
}

impl Context {
    pub fn client(&self) -> &ClientMetadata;
}
```

Rules:

- Missing data is `None`; the facade MUST NOT substitute presentation values such as `"unknown"`.
- Values describe the downstream client connection, not an origin subrequest.
- Unicode city and organization values are preserved.
- Original header names preserve runtime-provided order and spelling. If unavailable, the value is `None`; reconstructing it from normalized headers is forbidden.
- Provider-specific metadata MUST NOT be smuggled through request headers.
- Metadata is immutable and safe to clone into deferred tasks.

Minimum mapping:

| Common field | Cloudflare source | Fastly source |
|---|---|---|
| client IP | connecting-client address | downstream client IP API |
| POP | request colo | POP/datacenter API when available |
| geo | request continent/country/region/city/postal/metro/lat/long | client-IP geolocation |
| ASN/organization | request ASN metadata | network metadata when available |
| original header names | original-header metadata | downstream original-header API, otherwise `None` |
| JA3/JA4 | TLS/bot metadata | downstream TLS fingerprint APIs when available |
| cipher/extension hashes | request TLS metadata | downstream TLS metadata when available |

Applications derive any provider-neutral request headers from this structure.

## 6. Structured logging fields

Line logging and request-level fields are separate facilities:

```rust
impl Context {
    pub fn set_log_field(
        &self,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<()>;

    pub fn remove_log_field(&self, key: &str);
}
```

Contract:

- Fields are invocation-scoped strings.
- Keys are normalized to lowercase ASCII and validated against `[a-z0-9][a-z0-9._-]*`.
- Empty values are omitted; setting an existing key replaces it.
- Per-value and aggregate byte budgets are documented and enforced.
- Budget truncation is deterministic and emits a diagnostic.
- Fields are emitted for successful, synthetic, timeout, and catch-all responses.
- Origin responses cannot inject the platform logging control field.
- Logging fields are not client-visible.
- Applications own allowlists, sensitive-data classification, and schema generation.

Platform mapping:

- **Cloudflare:** serialize fields into the platform control response header with correct escaping, after stripping an origin-provided value.
- **Fastly:** emit one structured record to the configured log endpoint during finalization.
- **Tests:** expose the finalized map to the harness and verify no control data reaches the client.

## 7. KV-backed dictionaries

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

## 8. Scheduled events

Scheduled events support optional maintenance such as KV dictionary prewarming, cache refresh, or metrics aggregation. They are not required for ordinary KV reads.

```rust
pub struct ScheduledEvent {
    pub scheduled_at: SystemTime,
    pub cron: Option<String>,
}

pub type ScheduledHandler =
    fn(ScheduledEvent, Context) -> Pin<Box<dyn Future<Output = Result<()>> + Send>>;
```

Applications SHOULD put maintenance logic in a normal async function callable from both a scheduled entry and an authenticated HTTP maintenance route.

Delivery:

- **Cloudflare:** `#[edge::scheduled]` maps to the native scheduled event.
- **Fastly:** no native scheduled ABI is assumed. Deployment invokes the same function through an authenticated internal HTTP endpoint or explicitly configured scheduler.
- **edge-cli:** may generate Cloudflare cron configuration and Fastly maintenance-route metadata, but validation fails when a schedule lacks a Fastly delivery mechanism.

Delivery is at least once, so handlers MUST be idempotent. Overlapping invocations for one service and schedule are disabled unless explicitly configured.

## 9. Rate limiting

Cloudflare's native rate-limit bindings and Fastly's edge rate counters plus penalty boxes implement one application-facing contract:

```rust
pub enum RateLimitFailureMode {
    Open,
    Closed,
}

pub enum RateLimitFailure {
    Timeout,
    Platform,
}

pub struct RateLimitOptions {
    pub timeout: Duration,
    pub failure_mode: RateLimitFailureMode,
}

pub struct RateLimitOutcome {
    pub allowed: bool,
    pub estimated_rate: Option<u64>,
    pub failure: Option<RateLimitFailure>,
}

pub struct RateLimiter { /* named binding */ }

impl Context {
    pub fn rate_limiter(&self, name: &str) -> Result<RateLimiter>;
}

impl RateLimiter {
    pub async fn check(
        &self,
        key: &str,
        options: RateLimitOptions,
    ) -> RateLimitOutcome;
}
```

Rules:

- The facade returns an outcome; the application decides whether to deny, observe, sample, or exempt.
- Timeout and platform failure are distinguishable from an exceeded policy.
- Fail-open or fail-closed behavior is explicit.
- Empty keys are rejected rather than mapped to a global bucket.
- Names identify configured policies, not provider objects.
- A batch API SHOULD permit independent checks to run concurrently and return outcomes by caller label.

Shared configuration expresses total requests per period:

```toml
[rate_limits.login]
limit = 600
period_seconds = 60
mitigation_seconds = 120
```

Platform mapping:

- **Cloudflare:** generate a native rate-limit binding and invoke it by key.
- **Fastly:** use an edge rate counter and penalty box. Check the penalty box, update/read the counter, compare it with the normalized policy, and penalize an exceeded key for `mitigation_seconds`.
- The Fastly adapter owns conversion from total requests per period to the provider's rate representation.
- Exact algorithms may differ. The guarantee is policy-level: normal traffic is allowed, sustained excess is denied for the mitigation interval, and failures follow the selected mode.
- Config validation rejects periods or mitigation durations unsupported by either selected target.

## 10. Configuration

```toml
[runtime]
request_timeout_ms = 20000
max_deferred_tasks = 32
deferred_budget_ms = 30000

[logging]
endpoint = "default_logging"
max_fields_bytes = 98304

[[schedules]]
name = "dictionary-prewarm"
cron = "*/4 * * * *"
handler = "prewarm"
fastly_delivery = "authenticated_http"

[rate_limits.login]
limit = 600
period_seconds = 60
mitigation_seconds = 120
```

Application timeouts may be stricter than config but cannot extend a platform or request deadline. Maintenance-route credentials remain secret bindings and MUST NOT be emitted into generated config.

## 11. Conformance

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

## 12. Delivery order

Milestone ownership: see `SPEC.md` §12, M7–M14.

1. Wake-capable Fastly executor, monotonic clock, and deadlines.
2. Fetch timeout and client-disconnect behavior.
3. Deferred work.
4. Client metadata.
5. Structured logging fields.
6. Rate-limit adapters and code generation.
7. Optional scheduled delivery.

KV-backed dictionaries require no new facade primitive beyond the existing KV API. Caching and prewarming can be added independently after measurement.
