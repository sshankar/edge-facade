# M0 Implementation Plan — `edge-core` + native mock context

**Status: ✅ COMPLETE (2026-08-21)** — all acceptance criteria met: 32 tests green, both wasm targets check clean, clippy `-D warnings` clean, fmt clean. Final deviations below supersede the draft versions.

**Goal (spec §12, M0):** `edge-core` — platform-agnostic types, `Context` SPI, error model, router, logging — plus a native mock platform, such that conformance tests **T1, T2, T3, T7, T9** pass on the host.

**Definition of done:**
- `cargo test -p edge-core` green (T1, T2, T3, T7, T9 + unit tests).
- `cargo check --target wasm32-unknown-unknown` and `--target wasm32-wasip1` green for `edge-core` (proves the quarantine rule: core compiles on host + both wasm targets with zero platform deps).
- `clippy -D warnings`, `cargo fmt --check` clean.
- `#![forbid(unsafe_code)]` in `edge-core` (property worth enforcing from day one).
- No `#[edge::main]`, no adapters, no `edge-cli` — those are M1/M2.

---

## 1. Workspace scaffolding

```
edge/
├── Cargo.toml            # [workspace] resolver=2, members = ["edge-core"]
├── SPEC.md
├── PLAN-M0.md            # this file
└── edge-core/
    ├── Cargo.toml        # rust-version 1.88; no platform deps
    └── src/
        ├── lib.rs        # crate docs, re-exports, #![forbid(unsafe_code)]
        ├── types.rs      # §6.1
        ├── error.rs      # §6.4
        ├── context.rs    # §6.3 — Context struct + Platform SPI (sealed)
        ├── kv.rs         # §6.5 — KvStore/KvValue + KvBackend SPI
        ├── router.rs     # §6.6 — Router, RouteParams, Handler
        ├── log.rs        # info!/warn!/error! macros
        └── testing/
            └── mod.rs    # MockPlatform, MockContext, record types
```

- `edge-core` deps (only): `http` (1.x), `bytes` (1.x), `url` (2.x), `matchit` (0.8/0.7 — pick latest stable), `futures-util` (`default-features = false`, for `BoxFuture`), `serde` + `serde_json` (derive).
- Dev-deps: `tokio` (rt + macros, for `#[tokio::test]`).
- Toolchain prep (one-time): `rustup target add wasm32-unknown-unknown wasm32-wasip1`.

**SPI vs public API (implementation note):** spec §6.3 declares `pub struct Context { /* opaque */ }`. We implement it as `Context(Arc<dyn Platform>)` where `Platform` is a **sealed, crate-private** trait (the service-provider interface the adapters implement in M1/M2). Public surface stays exactly as specced; `testing::MockPlatform` implements the same trait. This gives us dependency injection for tests without leaking SPI.

## 2. API contracts to implement (from §6, with two recorded deviations)

### types.rs
```rust
pub type Body = bytes::Bytes;
pub type EdgeRequest = http::Request<Body>;
pub type EdgeResponse = http::Response<Body>;
pub use http::{Method, StatusCode, Version, HeaderMap, HeaderName, HeaderValue, Uri};
pub use url::Url;

impl EdgeResponse {
    pub fn ok(body: impl Into<Body>) -> Self;
    pub fn status(status: StatusCode, body: impl Into<Body>) -> Self;
    pub fn json<T: Serialize>(&mut self, value: &T) -> Result<()>;   // sets content-type: application/json
    pub fn text(&self) -> Result<&str>;                               // UTF-8 validated view, Body variant error on invalid
}
```

### error.rs
```rust
pub enum Error {
    Fetch(FetchError),
    Kv(KvError),
    Config(String),
    Router(PathError),
    Body(std::io::Error),
    Internal(String),
}
pub enum FetchError {
    UnresolvedBackend(String), Connection(String), Tls(String),
    Timeout, Permission, BadRequest(String), Platform(String),
}
pub enum KvError { Platform(String) }          // minimal for M0; grows with real adapters
pub enum PathError { NotFound, InvalidPattern(String) }  // router-internal
```
`From` impls for all inner → `Error`; `Display` + `std::error::Error`; `Error: Send + Sync + 'static`. No conversions to platform error types yet (adapter concern, M1/M2).

### context.rs
```rust
pub enum LogLevel { Info, Warn, Error }

pub struct Context(Arc<dyn Platform>);

impl Context {
    pub async fn fetch(&mut self, req: EdgeRequest) -> Result<EdgeResponse>;
    pub fn var(&self, name: &str) -> Option<String>;
    pub fn secret(&self, name: &str) -> Option<Vec<u8>>;
    pub fn kv(&self) -> KvStore;
    pub fn kv_named(&self, name: &str) -> Result<KvStore>;
    pub fn log(&self, level: LogLevel, message: &str);
}

trait Platform: Send + Sync {
    fn fetch(&self, req: EdgeRequest) -> BoxFuture<'_, Result<EdgeResponse>>;
    fn var(&self, name: &str) -> Option<String>;
    fn secret(&self, name: &str) -> Option<Vec<u8>>;
    fn kv(&self, name: &str) -> Result<KvStore>;
    fn log(&self, level: LogLevel, message: &str);
}
```

### kv.rs
```rust
pub struct KvStore(Arc<dyn KvBackend>);
impl KvStore {
    pub async fn get(&self, key: &str) -> Result<Option<KvValue>>;
    pub async fn put(&self, key: &str, value: impl Into<Body>) -> Result<()>;
    pub async fn delete(&self, key: &str) -> Result<()>;
}
pub struct KvValue(Body);
impl KvValue {
    pub async fn text(self) -> Result<Option<String>>;   // None on invalid UTF-8
    pub async fn bytes(self) -> Result<Body>;
    pub async fn json<T: DeserializeOwned>(self) -> Result<Option<T>>;
}
trait KvBackend: Send + Sync { /* mirror of the 3 ops, async */ }
```
Spec-parity: `get` → `Option`, not-found is `None` (D5.3).

### router.rs
```rust
pub struct Router { inner: matchit::Router<Handler> }
pub struct RouteParams { params: HashMap<String, String> }   // OWNED — deviation, see §5
impl RouteParams { pub fn get(&self, name: &str) -> Option<&str>; }

type Handler = Box<dyn Fn(EdgeRequest, RouteParams, &mut Context) -> BoxFuture<'_, Result<EdgeResponse>>>;

impl Router {
    pub fn new() -> Self;
    pub fn route(&mut self, pattern: &str, handler: Handler) -> Result<()>;
    pub async fn handle(&self, req: EdgeRequest, ctx: &mut Context) -> Result<EdgeResponse>;
    // 404 → Err(Error::Router(PathError::NotFound)); adapters convert to HTTP 404 in M1/M2
}
```
Convenience: `Router::get("/path", handler)` etc. wrappers. Non-`Send` futures (D7): handler returns `BoxFuture<'_, …>` without `Send` bound.

### log.rs
```rust
// ctx-first macros (deviation, see §5)
edge::log::info!(ctx, "handled {} in {:?}", path, elapsed);
edge::log::warn!(ctx, "…");
edge::log::error!(ctx, "…");
// → ctx.log(LogLevel::X, format!(...))
```

### testing/mod.rs
```rust
pub struct MockContext { /* builder */ }
pub struct MockContextBuilder {
    vars: HashMap<String, String>,
    secrets: HashMap<String, Vec<u8>>,
    kv: HashMap<String, Bytes>,               // default store
    kv_stores: HashMap<String, HashMap<String, Bytes>>,
    fetch_handler: Box<dyn Fn(EdgeRequest) -> Result<EdgeResponse> + Send + Sync>, // keyed by caller
    fail: MockFaults,                          // per-op failure injection
}
impl MockContextBuilder {
    pub fn new() -> Self;
    pub fn var(&mut self, name, value) -> &mut Self;
    pub fn secret(&mut self, name, value) -> &mut Self;
    pub fn kv_entry(&mut self, store, key, value) -> &mut Self;
    pub fn on_fetch(&mut self, f: impl Fn(EdgeRequest) -> Result<EdgeResponse> + Send + Sync + 'static) -> &mut Self;
    pub fn build(&self) -> MockContext;
}
// Records for assertions:
pub struct Records { pub logs: Vec<(LogLevel, String)>, pub fetches: Vec<EdgeRequest>, pub kv_ops: Vec<KvOp> }
impl MockContext { pub fn records(&self) -> &Records; }
```
The mock `fetch` mirrors the M3 backend-map shape (closure receives the full request, like a local origin) so T4-style tests port later.

## 3. Test plan (M0 acceptance + unit)

| Test | File | Asserts |
|---|---|---|
| T1 echo round-trip | `tests/t01_echo.rs` | handler echoes method, path, query, headers, body through `Router::handle`; mock records fetch-free path |
| T2 status/header/UTF-8 | `tests/t02_response.rs` | `EdgeResponse::ok/status/json/text`; header passthrough; multi-byte body round-trip |
| T3 router | `tests/t03_router.rs` | `/hello/:name` param extraction, 404 on unknown route, query extraction, method-specific routes |
| T7 vars/secrets | `tests/t07_config.rs` | configured keys returned; unknown → `None`; empty-string values preserved |
| T9 logging | `tests/t09_logging.rs` | `info!/warn!/error!` → sink; message formatting; levels recorded |
| KV smoke | `tests/t08_kv_mock.rs` | put/get/delete round trip on mock; `get` missing → `None`; fault injection → `KvError` (pre-validates T8 core logic) |
| Error unit | `src/error.rs` tests | Display strings; `From` conversions; `std::error::Error` source chaining |
| Types unit | `src/types.rs` tests | `text()` on invalid UTF-8 → `Error::Body`; `json` serialization + content-type |

## 4. Task sequence

1. **Scaffold** — workspace `Cargo.toml`, `edge-core/Cargo.toml` (deps, MSRV, lints), `rustup target add` both wasm targets, empty `lib.rs` compiles for all 3 targets.
2. **`error.rs`** — full enum tree + `From`s + Display/Error impls (types everything else returns).
3. **`types.rs`** — aliases, re-exports, `EdgeResponse` helpers + unit tests.
4. **`context.rs`** — sealed `Platform` SPI, `Context` wrapper, `LogLevel`.
5. **`kv.rs`** — `KvBackend` SPI, `KvStore`, `KvValue` + unit tests.
6. **`router.rs`** — `Router` over matchit, `RouteParams`, handler box type + unit tests.
7. **`log.rs`** — three macros.
8. **`testing/mod.rs`** — `MockContextBuilder`, records, fault injection.
9. **Integration tests** — T1, T2, T3, T7, T8-mock, T9.
10. **Gates** — host `cargo test`, both wasm `cargo check`, clippy, fmt; fix fallout.
11. **CI (optional but cheap)** — GitHub Actions: `test`, `check` × 2 wasm targets, clippy, fmt. Precedent: workers-rs has none in-tree; keep to 5-minute runtime.
12. **SPEC sync** — mark M0 done; record the three deviations below in §13.

## 5. Recorded deviations from SPEC (final, post-implementation; back-ported to SPEC §13 D8)

1. **`Platform` is public-but-`#[doc(hidden)]`, not sealed.** Adapters are separate crates and must implement the SPI; sealing would make that impossible. `Context::from_platform` / `KvStore::from_backend` are likewise `#[doc(hidden)]` public.
2. **`RouteParams` owns its map** (`HashMap<String, String>`) — borrowed params can't outlive the async handler boundary. Public semantics unchanged.
3. **Log macros take `ctx` as first argument** (`info!(ctx, …)`), exported as `edge_core::log::{info,warn,error}` (also `edge_core::log_info!` etc. at the root via `#[macro_export]`).
4. **`testing` module is always compiled** (no `feature = "testing"` gate): it's std-only and tiny; feature-gating would force self-dev-dependency or conformance-harness plumbing for zero benefit.
5. **Handler futures are `Send`** (`futures_util::future::BoxFuture`). Refinement of SPEC D7: core handlers only capture plain data, so their futures are genuinely `Send`; the `!Send` constraint applies only to adapter glue (JS objects), which never crosses into core.
6. **`ResponseExt::status(code, body)` renamed → `with_status`.** Inherent `http::Response::status(&self)` shadows the trait constructor when called via the type alias (inherent > trait in resolution). `ok`/`json`/`text` don't collide.
7. **`EdgeRequest::builder()` doesn't exist** — `http::Request::builder()` is defined on `Request<()>` only in http 1.x. Documented: use `http::Request::builder()`. (A core `RequestExt::builder()` helper is a future nicety, not needed for M0.)
8. **matchit 0.7.3 does not require a leading `/`** and rejects e.g. catch-all-not-at-end (`/foo/*rest/bar`); `InvalidPattern` maps those insert errors.

## 6. Risks (M0-sized)

- **matchit version/API drift** (0.7 → 0.8 breaking changes): pin exact version; `RouteParams` owns data so we're insulated from `Params` lifetime changes.
- **`BoxFuture` + non-`Send` + `matchit::Router<Handler>`**: `Handler` must be `Send + Sync` to live in a global-ish router? matchit doesn't require it, but `Router` may need to be shared across awaits in adapters → decide during task 6; worst case `Arc<Router>` + `Send+Sync` handler with interior mutability excluded (handlers take `&mut Context`, so handlers themselves can be plain `Fn`).
- **`http` 1.x vs `url` 2.x interop**: `http::Uri` → `Url` conversion is infallible-ish but needs care (`TryFrom`); centralize in `types.rs` with unit tests.
- No platform-specific risk in M0 by construction (quarantine rule makes it impossible to leak).

## 7. Explicitly out of scope for M0

`#[edge::main]` macro, both adapters, `edge-cli`, real fetch resolution (mock only), real KV, conformance harness drivers (Viceroy/workerd), T4/T5/T6/T10/T11, streaming, `Send`-safe layer, axum.

---

*Estimated: ~250–350 lines of core + ~400–500 of tests. Small, reviewable PRs: (1) scaffold+error+types, (2) context+kv+log, (3) router+testing, (4) tests+gates+CI.*
