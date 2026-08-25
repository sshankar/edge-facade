# Handler contract (§6.2)

Split from `SPEC.md` §6.2 (2026-08-25). Part of the [API surface](README.md).

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
- If `feature = "fastly"`: emits a sync `fn main() -> Result<(), fastly::Error>` that calls `fastly::Request::from_client()`, converts to `EdgeRequest`, drives the async handler via the adapter's poll-loop executor ([adapters/execution](../adapters/execution.md)), converts the response, and `send_to_client()`s it. Handler errors become a 500 with the error string (same convention as `fastly::main`).
- If both or neither feature is set: compile error with a clear message.

## See also

- [context](context.md) — the `Context` argument (§6.3)
- [errors](errors.md) — the `Result`/`Error` model (§6.4)
- [overview §6.2 expansion details](../overview.md)
