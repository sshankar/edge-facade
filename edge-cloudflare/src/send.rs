//! `Send` bridging for workers-rs futures (SPEC D8.6: adapter glue is
//! allowed to be `!Send`; the core SPI requires `Send` futures).
//!
//! Workers-rs async operations (`js_sys::futures::JsFuture` and the KV
//! builders) capture `Rc<RefCell<…>>` internally, so their futures are
//! `!Send`. The wasm runtime is single-threaded and the future is driven to
//! completion on the thread that created it, so marking these futures `Send`
//! is sound here — exactly the same justification the workers-rs crate uses
//! for its own `unsafe impl Send` on `Env`/`Request`/`Response`/`KvStore`.
//! The future is never sent to another thread; the marker only satisfies the
//! core SPI's `Send` bound.
//!
//! # Safety
//!
//! [`SendFuture`] is only constructed around futures that (a) execute on the
//! JS event loop of the thread that created them, and (b) are awaited to
//! completion in that same thread's poll loop. No worker is created, so the
//! value is never moved across threads.

/// Wrap a `!Send` future in a `Send` marker (see module docs).
#[derive(Debug)]
pub struct SendFuture<F>(pub F);

// SAFETY: see module docs — the wrapped future is created and driven to
// completion on the same (single) thread; it is never sent elsewhere.
#[allow(unsafe_code)]
unsafe impl<F> Send for SendFuture<F> {}

impl<F: std::future::Future> std::future::Future for SendFuture<F> {
    type Output = F::Output;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        // SAFETY: `SendFuture` is a transparent wrapper; projecting the pin
        // to the inner future does not move it, so structural pinning holds.
        #[allow(unsafe_code)]
        let inner = unsafe { self.map_unchecked_mut(|s| &mut s.0) };
        inner.poll(cx)
    }
}
