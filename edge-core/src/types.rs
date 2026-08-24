//! Common HTTP types and helpers.
//!
//! Bodies are either fully buffered [`Bytes`] or a streaming chunk source
//! (SPEC §6.1, decision D2 / D21). `Context::fetch` returns buffered bodies
//! (v1 semantics); `Context::fetch_streaming` returns streaming bodies.
//! Handlers return either to the client: adapters stream `Streaming` bodies
//! and send `Buffered` bodies whole.

use std::task::{self, Poll};

use bytes::Bytes;
use serde::Serialize;

use crate::Result;

/// A body: either fully buffered bytes, or an async source of chunks.
///
/// Construct with [`Body::buffered`], [`Body::stream`], [`Body::once`], or
/// [`Body::from_chunks`]; the `From` impls ([`Bytes`], `String`, `&str`,
/// `Vec<u8>`) always produce buffered bodies.
///
/// Reading:
/// - [`Body::next_chunk`] reads one chunk at a time (streaming processing);
/// - [`Body::collect`] drains the whole body into [`Bytes`] (buffering);
/// - [`Body::as_bytes`] is `Some` only for buffered bodies.
///
/// A `Buffered` body also behaves as a one-shot chunk source: the first
/// `next_chunk` yields its bytes, subsequent calls yield `None`. This lets a
/// partially-consumed body (or any body) be re-wrapped with
/// [`Body::stream`] and relayed as a stream.
pub enum Body {
    /// Fully buffered bytes (the v1 default; `Context::fetch` always returns
    /// this).
    Buffered(Bytes),
    /// An async source of chunks (from `Context::fetch_streaming`, or built
    /// by the handler with [`Body::stream`]).
    Streaming(Box<dyn ChunkStream>),
}

impl std::fmt::Debug for Body {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Body::Buffered(bytes) => f.debug_tuple("Buffered").field(bytes).finish(),
            Body::Streaming(_) => f.write_str("Streaming(..)"),
        }
    }
}

/// An async source of body chunks (SPEC D21).
///
/// Implement this to feed a custom stream into [`Body::stream`], or to wrap
/// a platform body handle in an adapter. [`ChunkStream::poll_next_chunk`]
/// follows the `Future`/`http_body` poll contract:
///
/// - `Poll::Ready(Ok(Some(chunk)))` — one chunk of body data;
/// - `Poll::Ready(Ok(None))` — end of stream;
/// - `Poll::Ready(Err(_))` — the stream failed;
/// - `Poll::Pending` — no data yet; wake `cx` when data arrives.
///
/// On Fastly, adapter chunk reads are blocking host calls, so they always
/// return `Ready` (compatible with the D3 poll-loop executor; no
/// select-scheduler needed — SPEC D21). On Cloudflare, reads are driven by
/// the JS event loop and may return `Pending`.
pub trait ChunkStream: Send + std::fmt::Debug + 'static {
    /// Poll for the next chunk.
    fn poll_next_chunk(&mut self, cx: &mut task::Context<'_>) -> Poll<Result<Option<Bytes>>>;
}

/// A buffered chunk list (from [`Body::from_chunks`]); yields each chunk in
/// order, then `None`.
#[derive(Debug)]
struct ChunkList {
    chunks: std::vec::IntoIter<Bytes>,
}

impl ChunkStream for ChunkList {
    fn poll_next_chunk(&mut self, _cx: &mut task::Context<'_>) -> Poll<Result<Option<Bytes>>> {
        Poll::Ready(Ok(self.chunks.next()))
    }
}

impl ChunkStream for Body {
    fn poll_next_chunk(&mut self, cx: &mut task::Context<'_>) -> Poll<Result<Option<Bytes>>> {
        Body::poll_next_chunk(self, cx)
    }
}

impl Body {
    /// An empty buffered body.
    pub fn new() -> Self {
        Body::Buffered(Bytes::new())
    }

    /// A buffered body from static bytes.
    pub fn from_static(bytes: &'static [u8]) -> Self {
        Body::Buffered(Bytes::from_static(bytes))
    }

    /// Wrap fully-buffered bytes.
    pub fn buffered(bytes: Bytes) -> Self {
        Body::Buffered(bytes)
    }

    /// Wrap a chunk source as a streaming body.
    ///
    /// `impl ChunkStream` includes [`Body`] itself, so a partially-consumed
    /// body can be re-wrapped and relayed (e.g. after reading a header
    /// chunk, stream the remainder to the client).
    pub fn stream(stream: impl ChunkStream) -> Self {
        Body::Streaming(Box::new(stream))
    }

    /// A one-shot streaming body yielding `bytes` as a single chunk, then
    /// `None`.
    pub fn once(bytes: Bytes) -> Self {
        Body::from_chunks(std::iter::once(bytes))
    }

    /// A streaming body yielding each chunk in order.
    pub fn from_chunks(chunks: impl IntoIterator<Item = Bytes>) -> Self {
        Body::stream(ChunkList {
            chunks: chunks.into_iter().collect::<Vec<_>>().into_iter(),
        })
    }

    /// Whether this is a streaming (unbuffered) body.
    pub fn is_streaming(&self) -> bool {
        matches!(self, Body::Streaming(_))
    }

    /// Whether this body is empty.
    ///
    /// Streaming bodies report `false` (their size is not known without
    /// reading).
    pub fn is_empty(&self) -> bool {
        match self {
            Body::Buffered(bytes) => bytes.is_empty(),
            Body::Streaming(_) => false,
        }
    }

    /// The bytes of a buffered body, or `None` for a streaming body.
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Body::Buffered(bytes) => Some(bytes),
            Body::Streaming(_) => None,
        }
    }

    /// Consume a buffered body into its bytes, or `None` for streaming.
    pub fn into_bytes(self) -> Option<Bytes> {
        match self {
            Body::Buffered(bytes) => Some(bytes),
            Body::Streaming(_) => None,
        }
    }

    /// Poll for the next chunk (see [`ChunkStream`]).
    pub fn poll_next_chunk(&mut self, cx: &mut task::Context<'_>) -> Poll<Result<Option<Bytes>>> {
        match self {
            Body::Buffered(bytes) => {
                if bytes.is_empty() {
                    Poll::Ready(Ok(None))
                } else {
                    Poll::Ready(Ok(Some(std::mem::take(bytes))))
                }
            }
            Body::Streaming(stream) => stream.poll_next_chunk(cx),
        }
    }

    /// Read the next chunk, or `None` at end of stream.
    pub async fn next_chunk(&mut self) -> Result<Option<Bytes>> {
        futures_util::future::poll_fn(|cx| self.poll_next_chunk(cx)).await
    }

    /// Read the whole body into memory.
    ///
    /// Returns the bytes immediately for buffered bodies; drains streaming
    /// bodies chunk by chunk. Use this when you need the full payload (or to
    /// store it, e.g. in KV).
    pub async fn collect(self) -> Result<Bytes> {
        match self {
            Body::Buffered(bytes) => Ok(bytes),
            Body::Streaming(mut stream) => {
                let mut out = Vec::new();
                loop {
                    let chunk =
                        futures_util::future::poll_fn(|cx| stream.poll_next_chunk(cx)).await?;
                    match chunk {
                        Some(chunk) => out.extend_from_slice(&chunk),
                        None => return Ok(out.into()),
                    }
                }
            }
        }
    }
}

impl Default for Body {
    fn default() -> Self {
        Body::new()
    }
}

impl From<Bytes> for Body {
    fn from(bytes: Bytes) -> Self {
        Body::Buffered(bytes)
    }
}

impl From<Vec<u8>> for Body {
    fn from(vec: Vec<u8>) -> Self {
        Body::Buffered(vec.into())
    }
}

impl From<String> for Body {
    fn from(s: String) -> Self {
        Body::Buffered(s.into())
    }
}

impl From<&str> for Body {
    fn from(s: &str) -> Self {
        Body::Buffered(s.as_bytes().to_vec().into())
    }
}

impl From<&'static [u8]> for Body {
    fn from(bytes: &'static [u8]) -> Self {
        Body::from_static(bytes)
    }
}

/// A platform-independent request.
pub type EdgeRequest = http::Request<Body>;

/// A platform-independent response.
pub type EdgeResponse = http::Response<Body>;

pub use http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri, Version};
pub use url::Url;

/// Convenience constructors/accessors for [`EdgeResponse`] (i.e.
/// `http::Response<Body>`).
///
/// Import with `use edge_core::ResponseExt;` to call `EdgeResponse::ok(..)`.
pub trait ResponseExt {
    /// Build a `200 OK` response with the given body.
    fn ok(body: impl Into<Body>) -> Self;

    /// Build a response with the given status and body.
    ///
    /// Named `with_status` to avoid colliding with the inherent
    /// [`http::Response::status`] accessor.
    fn with_status(status: StatusCode, body: impl Into<Body>) -> Self;

    /// Serialize `value` as JSON, setting `Content-Type: application/json`.
    fn json<T: Serialize>(&mut self, value: &T) -> crate::Result<()>;

    /// View the body as a UTF-8 string.
    ///
    /// Returns [`Error::Body`](crate::Error::Body) if the body is not valid
    /// UTF-8 or is streaming (collect it first with [`Body::collect`]).
    fn text(&self) -> crate::Result<&str>;
}

impl ResponseExt for EdgeResponse {
    fn ok(body: impl Into<Body>) -> Self {
        Self::with_status(StatusCode::OK, body)
    }

    fn with_status(status: StatusCode, body: impl Into<Body>) -> Self {
        http::Response::builder()
            .status(status)
            .body(body.into())
            // Cannot fail: no headers set, no invalid status provided.
            .expect("static response construction cannot fail")
    }

    fn json<T: Serialize>(&mut self, value: &T) -> crate::Result<()> {
        let bytes = serde_json::to_vec(value).map_err(json_err)?;
        self.headers_mut().insert(
            http::header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        *self.body_mut() = bytes.into();
        Ok(())
    }

    fn text(&self) -> crate::Result<&str> {
        match self.body() {
            Body::Buffered(bytes) => std::str::from_utf8(bytes).map_err(utf8_err),
            Body::Streaming(_) => Err(stream_err()),
        }
    }
}

fn json_err(e: serde_json::Error) -> crate::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, e).into()
}

fn utf8_err(e: std::str::Utf8Error) -> crate::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, e).into()
}

fn stream_err() -> crate::Error {
    std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "body is streaming; collect it first with `Body::collect`",
    )
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Error;
    use std::future::Future;

    #[test]
    fn ok_defaults_to_200_with_empty_body() {
        let resp = EdgeResponse::ok(Bytes::new());
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(resp.body().is_empty());
        assert!(!resp.body().is_streaming());
    }

    #[test]
    fn status_sets_status_and_body() {
        let resp = EdgeResponse::with_status(StatusCode::NOT_FOUND, "nope");
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        assert_eq!(resp.body().as_bytes(), Some(&b"nope"[..]));
    }

    #[test]
    fn json_sets_content_type_and_serializes() {
        #[derive(Serialize)]
        struct Payload {
            n: u32,
        }
        let mut resp = EdgeResponse::ok(Bytes::new());
        resp.json(&Payload { n: 7 }).unwrap();
        assert_eq!(resp.headers()["content-type"], "application/json");
        assert_eq!(resp.body().as_bytes(), Some(&b"{\"n\":7}"[..]));
    }

    #[test]
    fn text_returns_utf8_view() {
        let resp = EdgeResponse::ok("héllo 世界");
        assert_eq!(resp.text().unwrap(), "héllo 世界");
    }

    #[test]
    fn text_rejects_invalid_utf8() {
        let resp = EdgeResponse::ok(Bytes::from_static(&[0xff, 0xfe]));
        assert!(matches!(resp.text(), Err(Error::Body(_))));
    }

    #[test]
    fn text_rejects_streaming_bodies() {
        let resp = EdgeResponse::ok(Body::from_chunks([Bytes::from_static(b"x")]));
        assert!(matches!(resp.text(), Err(Error::Body(_))));
    }

    #[test]
    fn buffered_from_str_and_vec_are_buffered() {
        assert!(!Body::from("hi").is_streaming());
        assert!(!Body::from(vec![1u8, 2]).is_streaming());
        assert!(Body::from_static(b"static").as_bytes().is_some());
    }

    #[test]
    fn buffered_body_acts_as_one_shot_stream() {
        let mut body = Body::from_static(b"abc");
        assert_eq!(poll_ready(body.next_chunk()).unwrap().unwrap(), &b"abc"[..]);
        assert!(poll_ready(body.next_chunk()).unwrap().is_none());
    }

    #[test]
    fn from_chunks_yields_in_order_then_eof() {
        let mut body = Body::from_chunks([
            Bytes::from_static(b"a"),
            Bytes::from_static(b"bb"),
            Bytes::from_static(b"ccc"),
        ]);
        assert_eq!(poll_ready(body.next_chunk()).unwrap().unwrap(), &b"a"[..]);
        assert_eq!(poll_ready(body.next_chunk()).unwrap().unwrap(), &b"bb"[..]);
        assert_eq!(poll_ready(body.next_chunk()).unwrap().unwrap(), &b"ccc"[..]);
        assert!(poll_ready(body.next_chunk()).unwrap().is_none());
        assert!(body.is_streaming());
    }

    #[test]
    fn once_yields_single_chunk() {
        let mut body = Body::once(Bytes::from_static(b"x"));
        assert_eq!(poll_ready(body.next_chunk()).unwrap().unwrap(), &b"x"[..]);
        assert!(poll_ready(body.next_chunk()).unwrap().is_none());
    }

    #[test]
    fn collect_buffered_returns_bytes() {
        let body = Body::from("hello");
        assert_eq!(
            poll_ready(body.collect()).unwrap(),
            Bytes::from_static(b"hello")
        );
    }

    #[test]
    fn collect_streaming_drains_all_chunks() {
        let body = Body::from_chunks([
            Bytes::from_static(b"one,"),
            Bytes::from_static(b"two,"),
            Bytes::from_static(b"three"),
        ]);
        assert_eq!(
            poll_ready(body.collect()).unwrap(),
            Bytes::from_static(b"one,two,three")
        );
    }

    #[test]
    fn reboxing_relays_unread_chunks() {
        // Simulate "read a header chunk, then relay the rest": consume one
        // chunk, re-wrap the remainder as a new streaming body.
        let mut body = Body::from_chunks([
            Bytes::from_static(b"head|"),
            Bytes::from_static(b"body|"),
            Bytes::from_static(b"tail"),
        ]);
        let head = poll_ready(body.next_chunk()).unwrap().unwrap();
        assert_eq!(head, &b"head|"[..]);
        let relay = Body::stream(body);
        let rest = poll_ready(relay.collect()).unwrap();
        assert_eq!(rest, Bytes::from_static(b"body|tail"));
    }

    /// Drive a future to completion with a no-op waker (host-side tests only;
    /// every core future resolves without external wakeups).
    fn poll_ready<T>(fut: impl Future<Output = T>) -> T {
        let mut fut = Box::pin(fut);
        let waker = task::Waker::noop();
        let mut cx = task::Context::from_waker(waker);
        loop {
            if let Poll::Ready(out) = fut.as_mut().poll(&mut cx) {
                return out;
            }
        }
    }
}
