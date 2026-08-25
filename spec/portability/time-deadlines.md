# Time and deadlines (§4)

Split from `SPEC-PORTABILITY-PRIMITIVES.md` §4 (2026-08-25). Part of the
[portability primitives](README.md) (draft v0.2). Original section number
preserved as a heading so references ("§4") keep resolving.

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
