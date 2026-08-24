//! Minimal poll-loop executor for the async handler (SPEC §8.3).
//!
//! `#[fastly::main]` is synchronous and the SDK ships no executor. We drive
//! the handler future with a no-op waker, relying on the adapter invariant
//! that every `Context` method resolves on its first poll (each is a thin
//! `async` wrapper over blocking host calls). The loop therefore terminates;
//! a future that does return `Pending` (e.g. user code using `join!` over
//! adapter futures, which is unsupported on Fastly in v1) trips a spin
//! limit and panics with a clear message instead of hanging the instance.

use std::future::Future;
use std::task::{Context as TaskContext, Poll, Waker};

/// Maximum polls before declaring the no-Pending invariant violated.
const MAX_SPINS: u64 = 1_000_000;

/// Poll `fut` to completion.
pub fn drive<F: Future>(fut: F) -> F::Output {
    let mut fut = Box::pin(fut);
    let waker = Waker::noop();
    let mut cx = TaskContext::from_waker(waker);
    let mut spins = 0u64;
    loop {
        match fut.as_mut().poll(&mut cx) {
            Poll::Ready(out) => return out,
            Poll::Pending => {
                spins += 1;
                if spins > MAX_SPINS {
                    panic!(
                        "edge-fastly: handler future did not resolve after {MAX_SPINS} polls. \
                         The Fastly adapter (SPEC §8.3) resolves every await on its first poll; \
                         returning `Pending` means user code awaits concurrently (join!/select! \
                         over adapter futures), which is unsupported on Fastly in v1"
                    );
                }
            }
        }
    }
}
