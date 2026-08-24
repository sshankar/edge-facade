# Edge SDK Specification

**Status:** Draft v0.1
**Scope:** A Rust SDK for writing one worker/service that deploys to both Cloudflare Workers and Fastly Compute.
**Platform SDKs bridged:** `worker` (workers-rs) v0.8.5 · `fastly` v0.13.0

---

## 1. Purpose

Enable a single Rust handler, written once, to be compiled and deployed as:

| Platform | Wasm target | Runtime interface | Tooling |
|---|---|---|---|
| Cloudflare Workers | `wasm32-unknown-unknown` | JS host via wasm-bindgen (`worker` crate) | `worker-build`, `wrangler` |
| Fastly Compute | `wasm32-wasip1` / `wasm32-wasip2` | Pure WASI host ABI via `fastly-sys` | `fastly compute` |

The SDK provides:

1. A platform-agnostic core (types, handler contract, context, router, error model).
2. Per-platform adapters that convert between the core model and the native SDK.
3. A `#[edge::main]` entry macro that expands to the correct platform glue.
4. A shared configuration model and codegen so a single config produces both `wrangler.toml` and `fastly.toml`.

## 2. Non-goals (v1)

WebSockets, Durable Objects, Queues, R2, D1, Fanout, image optimizer, device detection, scheduled/cron events, streaming request/response bodies, platform-specific geo and cache APIs, HTTP/2 push, service bindings. Anything listed here is excluded unless a later version explicitly adopts it.

## 3. Ground truth — capability matrix (verified against SDK source)

| Capability | Cloudflare (`worker` 0.8.5) | Fastly (`fastly` 0.13.0) | Common abstraction |
|---|---|---|---|
| Handler entry | `#[event(fetch)]` on `async fn(req, env, ctx) -> Result<Response>` (must be async, 3 args) | `#[fastly::main]` on `fn main(req: Request) -> Result<Response, Error>` (sync, 1 arg) | `#[edge::main]` macro |
| Execution model | JS event loop (`wasm-bindgen-futures`, `future_to_promise`) | Sync entry; async via handle-based host I/O (`send_async` → `PendingRequest::wait()`); **no executor in SDK** | Async handler + adapter-driven execution (see §9.2) |
| HTTP interchange | `http::Request<worker::Body>` / `http::Response<B>` via `http` feature; helpers `request_from_wasm`/`request_to_wasm`/`response_from_wasm`/`response_to_wasm` | `From`/`Into` between `fastly::Request`/`Response` and `http::Request<Body>`/`http::Response<Body>` (request.rs:3066/3080, response.rs:2008/2019) | `http::Request<Bytes>` / `http::Response<Bytes>` |
| Subrequests | `Fetch::run` / `Fetcher::fetch_request` — absolute URL, any host | `Request::send(backend)` / `send_async` — named backend (static in `fastly.toml`, or dynamic via `Backend::builder`) | `Context::fetch` — URL-based, resolver maps URL→backend (§7) |
| Config vars | `Env` vars/secrets from `wrangler.toml` | `ConfigStore`, `Dictionary`, `SecretStore` | `Context::var` / `Context::secret` |
| KV | `env.kv("NS")` → `KvStore` (get/put/delete, metadata) | `KVStore` (lookup/insert/delete, async variants) | `Context::kv` (get/put/delete only) |
| Routing | Built-in `Router` (matchit) | none | core `Router` (matchit) |
| Logging | `console_log!` etc. (wrangler tail) | `fastly::log` endpoints (configurable) | `log::{info,warn,error}!` |
| Body | `worker::Body` implements `http_body::Body` (streaming) | `fastly::Body` handle, streaming-capable | fully buffered `Bytes` (v1) |

**Design consequences:**
1. The `http` crate is the lingua franca — both SDKs already convert to/from its types.
2. The async entry mismatch is the central adapter problem (§9.2).
3. Platform dependencies (wasm-bindgen vs fastly-sys) MUST be quarantined so the core compiles on host + both wasm targets.

## 4. Design principles

1. **URL-first, never backend-first.** Handlers express intent with URLs; platform transport details are adapter concerns.
2. **Buffered bodies in v1.** `Bytes` everywhere; streaming deferred to v2.
3. **Platform deps only in adapters.** `edge-core` depends only on `std` + `http` + `bytes` + `serde` + `serde_json` + `matchit` (+ `async-trait` if used).
4. **One handler, feature-selected platform.** `--features cloudflare` xor `--features fastly`; mutually exclusive.
5. **Behavioral parity, not API parity.** Where platforms differ semantically (Host header, redirects, KV limits), the SDK normalizes to the *stricter/most common* behavior and documents it.
6. **Fail closed on Fastly.** Undeclared fetch hosts are an error unless dynamic backends are explicitly enabled in config.

## 5. Workspace layout

```
edge/
├── Cargo.toml            # workspace
├── SPEC.md
├── edge-core/            # platform-agnostic: types, Context, Router, errors, config schema
├── edge-macros/          # #[edge::main] attribute macro
├── edge-cloudflare/      # adapter: depends on worker crate (wasm32-unknown-unknown only)
├── edge-fastly/          # adapter: depends on fastly crate (wasi targets only)
├── edge-cli/             # codegen: edge.toml → wrangler.toml + fastly.toml
├── examples/hello-world/ # one handler, both platforms
└── tests/conformance/    # shared conformance suite (see §11)
```

**Dependency rules (MUST):**
- `edge-core` compiles on host, `wasm32-unknown-unknown`, and `wasm32-wasip1`/`wasip2`.
- `edge-cloudflare` compiles only for `wasm32-unknown-unknown`; `edge-fastly` only for `target_env = "p1" | "p2"`. Enforced via `[target.'cfg(...)'.dependencies]`.
- `edge-fastly` pins `fastly = "=0.13.0"`, `fastly-macros = "=0.13.0"`, `fastly-sys = "=0.13.0"` (lockstep publishes).
- `edge-cloudflare` pins `worker = { version = "0.8.5", features = ["http"] }`.
- MSRV: 1.88 (Fastly's requirement).

## 6. Common API surface

### 6.1 HTTP types

```rust
// edge-core
pub type Body = bytes::Bytes;
pub type EdgeRequest = http::Request<Body>;
pub type EdgeResponse = http::Response<Body>;

pub use http::{Method, StatusCode, Version, HeaderMap, HeaderName, HeaderValue, Uri};
pub use url::Url;
```

Helper constructors (spec-level):

```rust
impl EdgeResponse {
    pub fn ok(body: impl Into<Body>) -> Self;
    pub fn status(status: StatusCode, body: impl Into<Body>) -> Self;
    pub fn json<T: Serialize>(&mut self, value: &T) -> Result<()>;
    pub fn text(&self) -> Result<&str>;   // UTF-8 validated view
}
```

Body size limits are platform limits; the adapter MUST document the effective limit (CF request/response limits vs Fastly limits) and MUST NOT silently truncate.

### 6.2 Handler contract

```rust
// edge-core
pub type Result<T> = std::result::Result<T, Error>;

/// User-facing handler. Identical source on both platforms.
#[edge::main]
async fn main(req: EdgeRequest, ctx: Context) -> Result<EdgeResponse> {
    // ...
}
```

`#[edge::main]` (from `edge-macros`) — expansion contract:
- If `feature = "cloudflare"`: emits the workers-rs fetch glue: a `pub fn fetch(web_sys::Request, Env, Context) -> js_sys::Promise` wrapper (pattern: `future_to_promise(AssertUnwindSafe(async move { ... }))`), converting request → `EdgeRequest`, building `Context::cloudflare(env)`, awaiting the handler, converting the response, mapping errors to `Response::error(..., 500)`.
- If `feature = "fastly"`: emits a sync `fn main() -> Result<(), fastly::Error>` that calls `fastly::Request::from_client()`, converts to `EdgeRequest`, drives the async handler via the adapter's poll-loop executor (§9.2), converts the response, and `send_to_client()`s it. Handler errors become a 500 with the error string (same convention as `fastly::main`).
- If both or neither feature is set: compile error with a clear message.

### 6.3 Context

```rust
pub struct Context { /* opaque; constructed only by adapters */ }

impl Context {
    // Subrequest (URL-based). See §7.
    pub async fn fetch(&mut self, req: EdgeRequest) -> Result<EdgeResponse>;

    // Config
    pub fn var(&self, name: &str) -> Option<String>;
    pub fn secret(&self, name: &str) -> Option<Vec<u8>>;

    // KV — default configured store; named variant for multi-store services
    pub fn kv(&self) -> KvStore;
    pub fn kv_named(&self, name: &str) -> Result<KvStore>;

    // Logging
    pub fn log(&self, level: LogLevel, message: &str);
}

pub enum LogLevel { Info, Warn, Error }
```

Logging convenience macros in `edge-core::log`:

```rust
edge::log::info!(...);   // → console_log! on CF; fastly log endpoint on Fastly
edge::log::error!(...);
```

On Fastly, the log destination is the endpoint named by `[logging] endpoint` in shared config, falling back to `eprintln!` (captured by Viceroy).

### 6.4 Error model

```rust
pub enum Error {
    // Common, semantics-normalized
    Fetch(FetchError),
    Kv(KvError),
    Config(String),
    Router(PathError),
    Body(std::io::Error),           // buffering failures
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
- CF: `TypeError`/fetch failures → `Connection | Tls | Timeout | Permission`.
- Fastly: `SendError`/`SendErrorCause` → `Connection | Tls | Timeout`; `BackendCreationError::Disallowed` → `Permission`; missing backend → `UnresolvedBackend`.
- The error type implements `std::error::Error`, `Display`, and `Into<worker::Error>` / `Into<fastly::Error>` for adapter boundaries.

### 6.5 KV

```rust
pub struct KvStore { /* opaque */ }

impl KvStore {
    pub async fn get(&self, key: &str) -> Result<Option<KvValue>>;
    pub async fn put(&self, key: &str, value: impl Into<Body>) -> Result<()>;
    pub async fn delete(&self, key: &str) -> Result<()>;
}

pub struct KvValue(/* opaque */);
impl KvValue {
    pub async fn text(self) -> Result<Option<String>>;   // None if invalid UTF-8? → use bytes
    pub async fn bytes(self) -> Result<Body>;
    pub async fn json<T: DeserializeOwned>(self) -> Result<Option<T>>;
}
```

Normalized semantics (parity decisions):
- `get` returns `Option`; not-found is `None` on both platforms.
- `put`/`delete` are fire-and-forget-ish; errors surface as `KvError`.
- TTL, metadata, and listing are **out of scope** in v1 (present on CF only).
- Documented limits: CF KV max value 25 MiB; Fastly KV value limit is smaller — adapters MUST document the effective limit; `put` MAY reject oversize values locally.

### 6.6 Router

```rust
pub struct Router;
pub struct RouteParams<'a> { /* extractor */ }

impl Router {
    pub fn new() -> Self;
    pub fn route(&mut self, pattern: &str, handler: Handler) -> Result<()>; // matchit patterns, e.g. "/hello/:name"
    pub async fn handle(&self, req: EdgeRequest, ctx: &mut Context) -> Result<EdgeResponse>; // 404 if unmatched
}
```

- Pattern syntax: matchit (same as workers-rs).
- No axum in v1 (§14).
- Router is a convenience; plain `match req.uri().path()` remains fully supported.

## 7. Fetch & backend resolution (URL → transport)

### 7.1 API contract

`Context::fetch(req)` takes a complete `http::Request` with an **absolute URI**. Relative URIs → `Error::Fetch(FetchError::BadRequest)`.

### 7.2 Shared origin config

Single source of truth, committed alongside the handler:

```toml
# edge.toml (schema v1)
[service]
name = "hello-world"

[origins]
api = { url = "https://api.example.com", backend = "api_backend" }

[stores]
kv = "edge_kv"                    # namespace name on CF, store name on Fastly

[logging]
endpoint = "default_logging"      # fastly log endpoint; ignored on CF

[fastly]
dynamic_backends = false          # MUST be explicit
```

`edge-cli` codegen produces, from this file:
- `fastly.toml`: `[setup.backends]` (`api_backend` with `override_host = "api.example.com"`, SSL on) and `[setup.kv_store]`/config-store entries.
- `wrangler.toml`: KV namespace binding `edge_kv`, vars, and the fetch permission/allowlist derived from `[origins]` (exact binding syntax per §14 open question 1).

The same map is embedded in the binary at build time (via a `build.rs`-generated module or `include_str!` + a `serde` deserializer) for runtime resolution. The runtime map is the **primary** resolution source on Fastly; it is authoritative and MUST match `fastly.toml`.

### 7.3 Resolution chain (Fastly adapter)

For `req.uri().host()` H, port P, scheme S:

1. **Static match:** if `origins[H]` exists → `Request::send(backend_name)` (validate with `Backend::from_name` for a friendly `UnresolvedBackend` error).
2. **Dynamic fallback:** else if `[fastly] dynamic_backends = true` → `Backend::builder(name, "H:P")` with:
   - `enable_ssl()` if S == https (SNI = H), plain if http
   - `override_host(H)` (parity requirement, §7.4)
   - `connect_timeout`/`first_byte_timeout`/`between_bytes_timeout` from defaults (configurable later)
   - `finish()`, handling `BackendCreationError::Disallowed` → `FetchError::Permission`
   - Cache per-session: `OnceLock<HashMap<String /*host*/, Backend>>` (dynamic backends are per-session entities; names may overlap across sessions, so per-session caching is correct).
3. **Else:** `FetchError::UnresolvedBackend(H)` (fail closed).

### 7.4 Behavioral parity rules (MUST)

1. **Host header / SNI identity:** CF sends the URL host upstream. Fastly connects to the backend address and uses backend host by default. The adapter MUST ensure the origin receives `Host: H` — via `override_host(H)` on dynamic backends and `override_host` in generated `fastly.toml` for static backends. Empirically verified in M3 (Viceroy + workerd echo-origin test).
2. **Redirects:** CF `fetch` follows by default; Fastly `send` does not. The adapter MUST set CF redirect policy to `manual` so redirect handling is identical (none) on both platforms.
3. **Path/query:** preserved verbatim; only transport differs.
4. **Request headers:** pass through except hop-by-hop normalization differences — MUST be documented and tested (e.g. `connection`, `keep-alive`).

### 7.5 Error behavior

- Unresolved host → `FetchError::UnresolvedBackend` (Fastly) vs pass-through behavior (CF). Handlers MUST NOT assume both platforms reach the origin for arbitrary undeclared hosts; the conformance suite tests the declared-origin path and the fail-closed path.

## 8. Adapter contracts

### 8.1 edge-cloudflare

Responsibilities:
- Convert `web_sys::Request` ⇄ `EdgeRequest` (method, URI, headers, buffered body) — prefer the `worker` crate `http`-feature helpers (`request_from_wasm`, `response_to_wasm`, …) and normalize bodies via `Body::bytes()`.
- Build `Context::cloudflare(env)`:
  - `var`/`secret` from `Env` bindings.
  - `kv()` from the configured namespace binding (name from embedded config).
  - `fetch`: build `web_sys::Request` from `EdgeRequest`, call `Fetcher::fetch_request`/`Fetch::run` with `redirect: manual`; map errors per §6.4.
  - `log` → `console_log!`/`console_warn!`/`console_error!`.
- Drive the handler on the JS event loop (async native — no executor needed).
- Never hold `!Send` JS objects across the handler boundary: everything crossing into `edge-core` is plain data (`Bytes`, `String`).

### 8.2 edge-fastly

Responsibilities:
- Convert `fastly::Request` ⇄ `EdgeRequest` via the `From`/`Into` `http` impls, then buffer the body (`read_to_end`).
- Build `Context::fastly()`:
  - `var`/`secret` from `ConfigStore`/`SecretStore` (names from embedded config).
  - `kv()` from the configured `KVStore`.
  - `fetch`: resolution chain §7.3; use `send` (sync) — body already buffered, so no streaming send needed in v1.
  - `log` → configured log endpoint (`fastly::log`), fallback `eprintln!`.
- **Drive async from the sync entry (§9.2).**
- Map errors per §6.4.

### 8.3 Execution of async on Fastly (the executor contract)

Facts: `#[fastly::main]` is sync; the SDK has no executor; async host I/O is handle-based (`send_async` → `PendingRequest::wait()` blocks; KV has sync + async variants); one instance per request.

Contract for v1:
- The Fastly adapter implements a minimal poll-loop "executor": `loop { poll(handler_future); if Ready → return; else → poll again }` with a waker that marks the task ready immediately.
- Invariant: every `Context` method on Fastly is implemented as an async fn whose body performs only blocking host calls and resolves on the first poll. Therefore futures never return `Pending` in practice, and the loop terminates.
- Consequence (documented constraint): **no concurrent awaits on Fastly in v1** — `join!`/`select!` over adapter futures is unsupported on Fastly (it works on CF). The conformance suite MUST include a sequential-await fetch test and MUST NOT include a concurrent one.
- If this contract proves fragile, revisit with a `fastly::async_io::select`-based scheduler (out of scope for v1).

## 9. Config & codegen

- Schema: §7.2. Extensions (vars, secrets) added in v1.1 — values live in platform configs, only *bindings* live in `edge.toml`.
- `edge-cli` commands:
  - `edge-cli generate` → writes `wrangler.toml` + `fastly.toml` from `edge.toml`.
  - `edge-cli check` → validates origin map matches deployed backends (parse `fastly.toml`, compare).
- Determinism: generated files MUST be reproducible and diffable.

## 10. Build & deploy

```text
# Cloudflare
cargo build --target wasm32-unknown-unknown --features cloudflare -p hello-world
worker-build (in examples/hello-world)      # JS shim
wrangler deploy

# Fastly
cargo build --target wasm32-wasip1 -p hello-world --features fastly
fastly compute deploy                        # fastly.toml generated by edge-cli
```

- Feature flags are mutually exclusive; both-on is a compile error (§6.2).
- Panic strategy: match platform conventions (wasm-bindgen/worker-build default on CF; Fastly default). Panics in the handler → 500 (same as both SDKs' conventions).

## 11. Conformance suite

A shared suite compiled natively (mock context), under Viceroy, and under workerd. Every test MUST pass identically on all three targets.

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

## 12. Milestones & acceptance criteria

| M | Deliverable | Exit criteria | Status |
|---|---|---|---|
| M0 | `edge-core` (types, Context trait, Error, Router) + native mock context | T1–T3, T7, T9 pass natively | ✅ done (2026-08-21) |
| M1 | `edge-fastly` adapter + `edge-macros` | hello-world + T1–T3 pass under Viceroy, then live Fastly | ✅ done (2026-08-21): T1–T4 + hello-world pass under Viceroy (see PLAN-M1); live Fastly deploy pending account access |
| M2 | `edge-cloudflare` adapter | hello-world + T1–T3 pass under workerd, then live CF | ✅ done (2026-08-22): T1–T4 + hello-world pass under workerd (see PLAN-M2); live CF deploy pending account access |
| M3 | Fetch resolver (static map + dynamic fallback) + parity rules | T4–T6, T11 on both platforms; Host parity verified empirically | resolver policy done (M1); parity verification pending |
| M4 | Config vars/secrets + KV | T7, T8 on both | — |
| M5 | Router, logging, `edge-cli`, conformance CI matrix, docs | full suite green on host + Viceroy + workerd; CI on both wasm targets | — |
| M6 (optional) | caching, geo, streaming bodies | — | — |

## 13. Decision log

Record of load-bearing design decisions. Each entry states the decision, the alternatives considered, the rationale (with references to SDK ground truth where relevant), the constraints it imposes, and the trigger that would reopen it.

### D1. URL-first fetch API (never backend-first)

- **Status:** Accepted
- **Decision:** `Context::fetch` takes a complete `http::Request` with an absolute URI. Handlers never name backends; the Fastly adapter resolves host → backend (§7.3).
- **Alternatives:** (a) `ctx.fetch_to("alias", path)` with handler-facing origin aliases; (b) passing a backend name through the common API.
- **Rationale:** CF only knows URLs, so a URL-based API maps to CF with zero loss. Alias/backend-first APIs leak deployment topology into handler code and break URL-proxying use cases (absolute links in rewritten HTML, user-supplied URLs, redirect targets). Fastly's request object carries a URL anyway; the backend is purely a transport detail, which is exactly what an adapter should own.
- **Consequences:** The Fastly adapter must own the host→backend map and the failure semantics for unmapped hosts (D4). Handlers targeting arbitrary undeclared hosts behave differently on the two platforms (§7.5) — documented, not hidden.
- **Revisit if:** A common use case emerges where URL-to-backend mapping cannot be expressed declaratively (e.g., hostnames derived dynamically with per-backend TLS material). Mitigation exists today via dynamic backends (D4).

### D2. Fully buffered `Bytes` bodies in v1 (no streaming)

- **Status:** Accepted
- **Decision:** `EdgeRequest`/`EdgeResponse` bodies are `bytes::Bytes`; bodies are buffered at the adapter boundary. Streaming is deferred (M6+).
- **Alternatives:** (a) `http_body::Body`-based common body type; (b) a custom streaming trait; (c) platform-native streaming without a common abstraction.
- **Rationale:**
  1. **No shared abstraction exists.** CF streaming is `worker::Body` implementing `http_body::Body` over `ReadableStream`; Fastly streaming is handle-based (`send_async_streaming`, `stream_to_client`) with **no `http_body::Body` impl** on `fastly::Body` (verified in `fastly/src/http/body.rs`). A common streaming model means writing our own trait + two adapters.
  2. **Streaming is incompatible with the D3 executor contract.** Streaming is inherently a `Pending` state machine; D3 requires adapter futures to resolve on the first poll. Honest streaming on Fastly requires the `select`-based scheduler we deferred.
  3. **Fake streaming is worse.** Without select-readiness, presenting an `http_body::Body` over a Fastly handle means read-ahead buffering — abstraction cost, no memory benefit.
  4. **Parity surface explodes.** Chunked vs content-length, trailers, mid-stream error timing, teardown — all become conformance obligations. Buffered bodies keep the T1–T11 suite honest.
  5. **v1 features don't need it.** Echo, fetch-relay, routing, KV, config. Non-goals (§2) already exclude the streaming-heavy features (WebSockets, Fanout).
- **Note:** Both platforms *have* streaming capability (`send_async_streaming`/`stream_to_client` on Fastly, `http_body::Body` on CF). The decision is to not expose it in the common API in v1, not to deny its existence.
- **Consequences:** Body size is bounded by platform limits; §6.1 requires adapters to document the effective limit and never silently truncate.
- **Revisit if:** Concrete demand emerges (large-file relay, SSE, media proxying). Enabler work: choose a streaming trait, build the Fastly select-scheduler (D3 revisit), adapt the Fastly handle to the trait, map CF `ReadableStream`.

### D3. Immediate-resolution async on Fastly (no executor in v1)

- **Status:** Accepted (with documented constraint)
- **Decision:** The Fastly adapter drives the async handler with a minimal poll-loop executor whose futures never return `Pending` in practice, because every `Context` method is implemented over blocking host calls (`send`/`wait`, sync KV).
- **Alternatives:** (a) Build a real `fastly::async_io::select`-based scheduler; (b) make the common handler synchronous and block only on CF.
- **Rationale:** `#[fastly::main]` is synchronous and the SDK ships no executor; its async model is handle-based (`send_async` → `PendingRequest::wait()` blocks). Fastly runs one instance per request, so per-request concurrency is not available to user code anyway. A CF-only-sync handler was rejected because CF `fetch` is inherently async (Promise-based) and cannot be blocked on synchronously.
- **Consequences (MUST be documented to users):** Concurrent awaits (`join!`, `select!`, `FuturesUnordered`) over adapter futures are unsupported on Fastly in v1; sequential awaits work. Code that runs on Fastly will also run on CF.
- **Revisit if:** Real per-request concurrency is needed on Fastly, or streaming (D2) lands — both require the select-scheduler.

### D4. Fail-closed backend resolution on Fastly

- **Status:** Accepted
- **Decision:** The Fastly adapter resolves fetch hosts in order: (1) static backend from the origin map, (2) dynamic backend via `Backend::builder` **only if** `[fastly] dynamic_backends = true` in shared config, (3) `FetchError::UnresolvedBackend`. Dynamic backends are opt-in, never implicit.
- **Alternatives:** (a) Always fall back to dynamic backends; (b) require all origins to be declared or error.
- **Rationale:** Dynamic backends are a per-service feature (`BackendCreationError::Disallowed` exists), carry per-session creation cost, and silent fallback would mask configuration drift between `edge.toml` and `fastly.toml`. Fail-closed keeps production behavior deterministic and makes undeclared-host fetches a visible error.
- **Consequences:** Handlers that fetch arbitrary user-controlled hosts will not work on Fastly unless dynamic backends are explicitly enabled; §7.5 documents the CF/Fastly asymmetry.
- **Revisit if:** Dynamic backends become universal (no `Disallowed`), or per-session creation cost drops to zero.

### D5. Behavioral parity by normalization (Host identity, redirects, KV not-found)

- **Status:** Accepted
- **Decision:** Where platform semantics diverge, the SDK normalizes to a single observable behavior rather than exposing the difference:
  1. **Host/SNI identity:** the origin receives `Host: <URL host>` on both platforms — via `override_host` on Fastly (dynamic backends) and `override_host` in generated `fastly.toml` (static backends); CF is natural.
  2. **Redirects:** CF `fetch` is set to `redirect: manual` so no platform auto-follows (Fastly never does).
  3. **KV not-found:** `get` returns `Option`, `None` on both platforms.
- **Alternatives:** Pass-through with per-platform documentation; opt-in flags per behavior.
- **Rationale:** v1's value proposition is one handler with identical observable behavior; normalization beats conditional handler code. Each normalization is individually small and testable (T4, T6, T8).
- **Consequences:** Loses CF's default redirect-following convenience (mitigated later by an opt-in `follow_redirects` implemented in core, not per adapter).
- **Revisit if:** A normalization proves to be a functional loss rather than a cosmetic one (measured via conformance failures).

### D6. Single source of truth config (`edge.toml`) with codegen

- **Status:** Accepted
- **Decision:** One `edge.toml` (§7.2) is the authoritative config. `edge-cli generate` produces `wrangler.toml` and `fastly.toml`; `edge-cli check` validates the origin map against deployed backends; the same map is embedded in the binary for runtime resolution.
- **Alternatives:** (a) Two hand-maintained platform configs plus runtime env injection; (b) runtime-only resolution via Config Store/Env.
- **Rationale:** Hand-maintaining two configs guarantees drift (the exact failure D4 exists to catch). Build-time embedding keeps resolution deterministic and free of platform setup order. Runtime override remains possible per environment (values in platform configs, bindings in `edge.toml`).
- **Consequences:** `edge-cli` becomes a required part of the build pipeline (§10).
- **Revisit if:** Platform config surface diverges faster than codegen can keep up; then consider runtime-only resolution.

### D7. Own matchit router, no axum, non-`Send` futures in v1

- **Status:** Accepted
- **Decision:** `edge-core` ships a small router on matchit patterns; axum is excluded; handler futures are not required to be `Send`.
- **Alternatives:** (a) axum as the routing layer; (b) no router, manual matching only.
- **Rationale:** axum requires `Send` futures; wasm-bindgen objects are `!Send` (CF side), so axum compatibility would force `SendWrapper`-style gymnastics at the boundary for zero v1 benefit. matchit is already proven in workers-rs. All data crossing into `edge-core` is plain (`Bytes`, `String`), so core types are `Send`/`Sync` regardless; only the async handler future is not required to be.
- **Consequences:** Frameworks like axum cannot be dropped into handlers in v1; plain `http` types keep the door open for a v2 `Send`-safe layer.
- **Revisit if:** Users demand framework integration; then adopt `Send` bounds + the necessary wrappers (per §14 open question 6).

### D8. API-shape refinements from M0 implementation

- **Status:** Accepted (recorded post-M0)
- **Decision:** The following concrete API shapes, confirmed during M0 implementation, refine the §6 sketches:
  1. `Platform` / `KvBackend` are public-but-`#[doc(hidden)]` traits (not sealed) — adapter crates must implement them; sealing would forbid that.
  2. `Context` wraps `Arc<dyn Platform>`; handlers receive `Context` **by value** (cheap clone), while `Router::handle(&self, req, ctx: &mut Context)` keeps the spec signature. `RouteParams` owns its map (borrowed params can't outlive async handlers).
  3. `ResponseExt::status(code, body)` is named **`with_status`** — the inherent `http::Response::status(&self)` accessor shadows trait constructors via the type alias.
  4. `http::Request::builder()` only exists on `Request<()>` in http 1.x — user code must write `http::Request::builder()`, not `EdgeRequest::builder()`.
  5. Log macros take the context as first argument and are exported as `edge_core::log::{info,warn,error}`.
  6. Core handler futures are `Send` (refinement of D7): core captures only plain data; `!Send` applies only to adapter glue.
  7. The `testing` module is always compiled (std-only, negligible); no feature gate.
- **Rationale:** Each refinement resolves a concrete collision or lifetime issue found while implementing M0 (see PLAN-M0 §5 for details).
- **Consequences:** §6.1/§6.3/§6.6 sketches are superseded by the implemented signatures in `edge-core` (the implementation is the reference for M1/M2).
- **Revisit if:** A future breaking release of `http` changes builder placement or adds inherent methods colliding with `ResponseExt`.

### D9. `edge.toml` embedding via the entry macro (no build.rs)

- **Status:** Accepted (recorded post-M1)
- **Decision:** `#[edge_core::main]`'s fastly branch embeds the config with `include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/edge.toml"))`, evaluated in the service crate. No build.rs; the runtime map is compile-time deterministic (§7.2).
- **Alternatives:** (a) build.rs-generated module in each service; (b) `edge-cli`-emitted `include!` file.
- **Rationale:** Zero build pipeline; the macro already exists per service. `CARGO_MANIFEST_DIR` + `include_str!` in macro output resolve in the downstream crate.
- **Consequences:** `edge.toml` must sit at the service crate root; trybuild cannot exercise the fastly-positive path (no `edge.toml` in its temp project) — hello-world/conformance builds cover it.
- **Revisit if:** A service needs multiple configs or dynamic override; then adopt build.rs + `EDGE_CONFIG_STR` env override.

### D10. Resolution policy in `edge-core` (`config::Resolution`)

- **Status:** Accepted (recorded post-M1)
- **Decision:** The §7.3 chain (static → dynamic-iff-enabled → fail-closed) is pure decision logic in `edge_core::config::Resolution`; the Fastly adapter maps decisions to `Backend::from_name`/`Backend::builder` + a per-session cache. `edge-core` gains a `toml` dependency (first addition to the §3 list; no platform deps — quarantine intact).
- **Alternatives:** Keep the whole chain in `edge-fastly` (untestable on host — see D14); duplicate decision logic in the CF adapter.
- **Rationale:** `dynamic_backends` is a shared-config property, and the decision is the part that needs host tests; transport differs per platform.
- **Consequences:** `edge-core` now depends on `toml`; `Resolution` is public (used by adapters).
- **Revisit if:** CF ever needs a fail-closed mode; the policy would move behind a per-platform flag.

### D11. `[stores]` config/secrets bindings in the schema

- **Status:** Accepted (recorded post-M1)
- **Decision:** `edge.toml` `[stores]` gains `config` and `secrets` binding names (values live in platform configs per §9; only bindings live in `edge.toml`). `[fastly] dynamic_backends` remains required (MUST be explicit, D4).
- **Rationale:** The Fastly adapter needs the ConfigStore/SecretStore names at runtime (§8.2) and there was no other sanctioned place.
- **Consequences:** The v0.1 schema sketch is superseded by `edge_core::config` (implementation is the reference).
- **Revisit if:** §9's v1.1 extension (vars/secrets in `edge.toml`) lands; bindings move to platform configs.

### D12. Adapter-level error handling (404 for router misses; infallible serve)

- **Status:** Accepted (recorded post-M1)
- **Decision:** `edge_fastly::serve` never returns `Err` in practice — every failure becomes a client response. `Error::Router(PathError::NotFound)` → `404` (fulfilling PLAN-M0 §2); all other handler errors → `500` with the error string (`fastly::main` convention, §6.2). Generated `fn main() -> std::result::Result<(), edge_fastly::Error>` keeps the spec shape (`edge_fastly::Error` re-exports `fastly::Error` = anyhow).
- **Alternatives:** Propagate config/context errors out of `main`; convert 404 in user code.
- **Rationale:** One handler must behave identically on both platforms (D5); `fastly::main`'s own contract is 500-on-error. 404-on-miss is a router semantic the adapters own.
- **Consequences:** Handler code never branches on platform for error/404 handling.
- **Revisit if:** A user needs custom error→status mapping; then expose `serve`-level error hooks.

### D13. Service crates declare both features; CF branch emitted but inert

- **Status:** Accepted (recorded post-M1)
- **Decision:** Service crates declare `cloudflare = []` (empty) alongside `fastly = ["dep:edge-fastly"]` so the macro's `#[cfg(feature = ...)]` gates are known to cargo; the macro emits the §6.2 cloudflare branch referencing `edge_cloudflare` (inert until M2).
- **Alternatives:** Declare only the implemented feature (silences nothing — `unexpected_cfgs` on the other gate); omit the CF branch (breaks the both-features error path).
- **Rationale:** Keeps the feature matrix honest and warning-free; enabling `cloudflare` before M2 is a compile error, which is the correct signal.
- **Revisit if:** M2 lands; `cloudflare = ["dep:edge-cloudflare"]` and the CF branch is finalized.

### D14. `edge-fastly` has no host-run tests (link constraint)

- **Status:** Accepted (recorded post-M1)
- **Decision:** The `fastly` crate's hostcalls (e.g. `register_dynamic_backend`) are undefined symbols on native; once any test reaches them, the test binary fails to link. `edge-fastly` therefore has zero host tests; policy logic is tested in `edge-core`, transport under Viceroy.
- **Alternatives:** (a) Mock/feature-gate hostcalls in `fastly` (fork — rejected); (b) viceroy-as-test-runner for adapter unit tests (overkill for M1).
- **Consequences:** New adapter logic should be pushed into testable positions (core) or covered by conformance scenarios.
- **Revisit if:** A host-side test harness for Compute (like nextest+viceroy) is adopted in M5's CI matrix.

### D15. Viceroy config: hand-maintained `[local_server]` + `[setup]` until edge-cli

- **Status:** Accepted (recorded post-M1)
- **Decision:** fastly.toml files are hand-written for M1 with both `[local_server]` (read by Viceroy: backends with `override_host`, config/kv/secret stores) and `[setup]` (deploy-time bootstrapping; edge-cli's codegen target in M5). Viceroy accepts arbitrary log-endpoint names (prints them prefixed); the adapter still falls back to `eprintln!`.
- **Alternatives:** Vendor a config generator now (M5 scope); drive Viceroy with CLI flags only (no backends/stores).
- **Rationale:** M1 needs Viceroy to resolve a declared backend with `override_host` for T4 (D5.1); `[local_server]` is the supported mechanism.
- **Consequences:** `[setup]` here is best-effort; M5's `edge-cli check` (origin map vs deployed backends) supersedes it.

### D16. CF fetch errors map to `Connection` (no typed causes in JS)

- **Status:** Accepted (recorded post-M2)
- **Decision:** Cloudflare fetch rejections are JS `Error` objects without typed causes, so the adapter maps them to `FetchError::Connection(js_message)`. `Permission`/`Tls`/`Timeout` are not produced on CF in v1 (no allowlist enforcement; TLS/timeout failures are indistinguishable from other network rejections).
- **Alternatives:** String-match on JS messages (fragile); expose raw `JsValue` (leaks platform into the error model).
- **Rationale:** The normalized model must stay platform-free; CF genuinely cannot provide Fastly's granularity.
- **Consequences:** Handlers that branch on `FetchError::Timeout` get the fine-grained behavior on Fastly and the coarse `Connection` on CF (SPEC §7.5 asymmetry, documented).
- **Revisit if:** Cloudflare exposes typed fetch errors in the runtime API.

### D17. Empty-body normalization (CF GET/HEAD)

- **Status:** Accepted (recorded post-M2)
- **Decision:** `web_sys::Request` throws `TypeError: Request with a GET or HEAD method cannot have a body` for a present-but-empty stream (verified under workerd, T4). The adapter maps empty `Bytes` to a null body (`worker::Body::empty()`).
- **Alternatives:** Always use `Body::empty()` for empty payloads (same thing, less explicit); carry body-presence in the core type (D2 refactor).
- **Rationale:** Buffered bodies mean empty is empty on both platforms; a null body is the wire-level parity.
- **Consequences:** None for handlers — empty stays empty.
- **Revisit if:** Streaming bodies land (D2 revisit); body-presence becomes a core concept.

### D18. `Send` bridging for workers-rs futures

- **Status:** Accepted (recorded post-M2)
- **Decision:** `JsFuture`-based workers-rs futures capture `Rc<RefCell<…>>` and are `!Send`; the core SPI requires `Send` futures. `edge-cloudflare` wraps them in a documented unsafe `Send` marker (`SendFuture`), sound because the wasm runtime is single-threaded and the future never leaves its creating thread.
- **Alternatives:** Relax `Send` in the SPI (would ripple into `Platform: Send + Sync` and the router's handler bounds); adopt `SendWrapper` (same unsafe, more machinery).
- **Rationale:** Matches the workers-rs crate's own `unsafe impl Send` convention and keeps core `Send` (D8.6: plain data only).
- **Consequences:** The unsafe marker is localized to `edge-cloudflare::send` with a safety note; the future is never actually moved across threads.
- **Revisit if:** A threaded host (Javy-style) appears; then the marker must be re-justified or the SPI relaxed.

### D19. Service crates depend on `wasm-bindgen` directly

- **Status:** Accepted (recorded post-M2)
- **Decision:** The `#[wasm_bindgen]` macro's generated code references `::wasm_bindgen`, so the service crate must list it as a direct dependency (optional, wired under the `cloudflare` feature; same semver as `worker`'s copy).
- **Alternatives:** Re-export through `edge-cloudflare` (does not add to the extern prelude); generate the glue without wasm-bindgen (would reimplement the ABI).
- **Rationale:** This is the standard wasm-bindgen requirement; one line per service crate.
- **Consequences:** Service crates have three direct deps (edge-core + the platform adapter + wasm-bindgen).
- **Revisit if:** The macro switches to emitting ABI code that does not require the crate (unlikely).

### D20. Service crates: cdylib lib + thin fastly bin

- **Status:** Accepted (recorded post-M2)
- **Decision:** `worker-build` requires a `cdylib` lib target; wasip1 needs a bin entry. Service crates ship a shared lib (handler + `#[edge_core::main]` under `--features cloudflare`, exporting `fetch` from the cdylib) and a ~6-line fastly bin with its own `#[edge_core::main]`. One handler source, two platform entries; the generated fastly `main` stays private (bin-local).
- **Alternatives:** Two entry files with duplicated handler wiring; build-time codegen that generates the lib/bin skeleton.
- **Rationale:** worker-build is the Cloudflare toolchain; the split is small and keeps the spec's "one handler" property.
- **Consequences:** `cargo build -p hello-world` (no features) fails on the lib's missing feature — the feature-matrix compile error, as designed; `default-members` excludes service crates.
- **Revisit if:** worker-build learns bin support; then the split collapses.

## 14. Open questions & risks

1. **CF fetch permission syntax:** current `wrangler.toml` binding for fetch allowlist/`unsafe` fetch (v1.3+ permission model) — verify and encode in `edge-cli`.
2. **Fastly Host/SNI semantics:** exact behavior of `send()` w.r.t. `Host` header when `override_host` unset — must be empirically confirmed in M3 (Viceroy echo origin).
3. **Dynamic backends:** per-service enablement; creation cost per session; whether `enable_pooling`/`max_use` should be surfaced in config.
4. **KV limits:** exact Fastly KV value size limit; decide local reject vs pass-through.
5. **MSRV drift:** workers-rs 1.75 vs fastly 1.88 → SDK MSRV 1.88; revisit if fastly raises.
6. **Send/Sync policy:** v1 requires plain-data boundaries so core types are `Send`/`Sync`; handler futures remain non-`Send` (matches both platforms' single-threaded reality; axum reuse later would require revisiting).
7. **Package naming:** `edge` may collide on crates.io; final names TBD (`edge-kit`, `duo-edge`, …).
8. **Redirect parity:** choosing "manual on CF" loses CF's convenience; documented tradeoff, revisit in v2 with an opt-in `follow_redirects` flag implemented in core.
