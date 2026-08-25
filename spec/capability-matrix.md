# Capability matrix (§3)

Ground truth per platform, verified against SDK source (`worker` 0.8.5,
`fastly` 0.13.0). Split from `SPEC.md` §3 (2026-08-25).

| Capability | Cloudflare (`worker` 0.8.5) | Fastly (`fastly` 0.13.0) | Common abstraction |
|---|---|---|---|
| Handler entry | `#[event(fetch)]` on `async fn(req, env, ctx) -> Result<Response>` (must be async, 3 args) | `#[fastly::main]` on `fn main(req: Request) -> Result<Response, Error>` (sync, 1 arg) | `#[edge::main]` macro |
| Execution model | JS event loop (`wasm-bindgen-futures`, `future_to_promise`) | Sync entry; async via handle-based host I/O (`send_async` → `PendingRequest::wait()`); **no executor in SDK** | Async handler + adapter-driven execution (see [adapters/execution](adapters/execution.md)) |
| HTTP interchange | `http::Request<worker::Body>` / `http::Response<B>` via `http` feature; helpers `request_from_wasm`/`request_to_wasm`/`response_from_wasm`/`response_to_wasm` | `From`/`Into` between `fastly::Request`/`Response` and `http::Request<Body>`/`http::Response<Body>` (request.rs:3066/3080, response.rs:2008/2019) | `http::Request<Bytes>` / `http::Response<Bytes>` |
| Subrequests | `Fetch::run` / `Fetcher::fetch_request` — absolute URL, any host | `Request::send(backend)` / `send_async` — named backend (static in `fastly.toml`, or dynamic via `Backend::builder`) | `Context::fetch` — URL-based, resolver maps URL→backend ([fetch](fetch.md)) |
| Config vars | `Env` vars/secrets from `wrangler.toml` | `ConfigStore`, `Dictionary`, `SecretStore` | `Context::var` / `Context::secret` |
| KV | `env.kv("NS")` → `KvStore` (get/put/delete, metadata) | `KVStore` (lookup/insert/delete, async variants) | `Context::kv` (get/put/delete only) |
| Routing | Built-in `Router` (matchit) | none | core `Router` (matchit) |
| Logging | `console_log!` etc. (wrangler tail) | `fastly::log` endpoints (configurable) | `log::{info,warn,error}!` |
| Body | `worker::Body` implements `http_body::Body` (streaming) | `fastly::Body` handle, streaming-capable | `Body` enum: `Buffered(Bytes)` (default; `fetch` returns this) or `Streaming(ChunkStream)` (`fetch_streaming`, M6/D21) |

**Design consequences:**
1. The `http` crate is the lingua franca — both SDKs already convert to/from its types.
2. The async entry mismatch is the central adapter problem ([adapters/execution](adapters/execution.md)).
3. Platform dependencies (wasm-bindgen vs fastly-sys) MUST be quarantined so the core compiles on host + both wasm targets.
