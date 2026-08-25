# Deferred work (§3)

Split from `SPEC-PORTABILITY-PRIMITIVES.md` §3 (2026-08-25). Part of the
[portability primitives](README.md) (draft v0.2). Original section number
preserved as a heading so references ("§3") keep resolving.

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
