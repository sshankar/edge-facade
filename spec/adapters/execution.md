# Execution of async on Fastly (§8.3 — the executor contract)

Split from `SPEC.md` §8.3 (2026-08-25). Part of the [adapter contracts](README.md).
Superseded for M7+ by the wake-capable executor — see
[portability/time-deadlines](../portability/time-deadlines.md).

Facts: `#[fastly::main]` is sync; the SDK has no executor; async host I/O is handle-based (`send_async` → `PendingRequest::wait()` blocks; KV has sync + async variants); one instance per request.

Contract for v1:
- The Fastly adapter implements a minimal poll-loop "executor": `loop { poll(handler_future); if Ready → return; else → poll again }` with a waker that marks the task ready immediately.
- Invariant: every `Context` method on Fastly is implemented as an async fn whose body performs only blocking host calls and resolves on the first poll. Therefore futures never return `Pending` in practice, and the loop terminates.
- Consequence (documented constraint): **no concurrent awaits on Fastly in v1** — `join!`/`select!` over adapter futures is unsupported on Fastly (it works on CF). The conformance suite MUST include a sequential-await fetch test and MUST NOT include a concurrent one.
- If this contract proves fragile, revisit with a `fastly::async_io::select`-based scheduler (out of scope for v1).

Streaming response bodies (M6) fit this contract because sequential chunk
reads are blocking host calls — see [D21](../decisions/d21.md).

## See also

- [D3](../decisions/d03.md) — immediate-resolution async in v1 (superseded for M7+)
- [portability/time-deadlines](../portability/time-deadlines.md) — the M7 wake-capable executor that supersedes this
- [capability-matrix](../capability-matrix.md) — the async mismatch is the central adapter problem
