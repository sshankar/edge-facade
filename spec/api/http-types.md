# HTTP types (§6.1)

The common body and request/response types shared by every platform.
Split from `SPEC.md` §6.1 (2026-08-25). The body model is the product of
two decisions: buffered-by-default [D2](../decisions/d02.md), superseded for
*response* bodies by streaming [D21](../decisions/d21.md) (request bodies stay
buffered).

## 6.1 HTTP types

```rust
// edge-core
pub enum Body {
    Buffered(bytes::Bytes),          // default; Context::fetch returns this
    Streaming(Box<dyn ChunkStream>), // Context::fetch_streaming, or handler-built
}

pub trait ChunkStream: Send + Debug + 'static {
    fn poll_next_chunk(&mut self, cx: &mut task::Context<'_>)
        -> Poll<Result<Option<Bytes>>>;   // Some(chunk) | None (EOF) | Err | Pending
}

pub type EdgeRequest = http::Request<Body>;
pub type EdgeResponse = http::Response<Body>;

pub use http::{Method, StatusCode, Version, HeaderMap, HeaderName, HeaderValue, Uri};
pub use url::Url;
```

Helper constructors (spec-level):

```rust
impl Body {
    pub fn buffered(bytes: Bytes) -> Self;             // wrap buffered bytes
    pub fn stream(impl ChunkStream) -> Self;           // wrap a chunk source
    pub fn once(bytes: Bytes) -> Self;                 // one-shot streaming body
    pub fn from_chunks(impl IntoIterator<Item = Bytes>) -> Self;
    pub fn as_bytes(&self) -> Option<&[u8]>;           // Some for Buffered only
    pub async fn next_chunk(&mut self) -> Result<Option<Bytes>>;
    pub async fn collect(self) -> Result<Bytes>;       // drain to Bytes
}

impl EdgeResponse {
    pub fn ok(body: impl Into<Body>) -> Self;
    pub fn status(status: StatusCode, body: impl Into<Body>) -> Self;
    pub fn json<T: Serialize>(&mut self, value: &T) -> Result<()>;
    pub fn text(&self) -> Result<&str>;   // UTF-8 validated view; errors on streaming bodies
}
```

A `Buffered` body also behaves as a one-shot chunk source, so any body can be
re-wrapped with `Body::stream` and relayed as a stream (T12). KV values are
always buffered bytes: `KvStore::put` drains streaming bodies ([D21](../decisions/d21.md)).

Body size limits are platform limits; the adapter MUST document the effective limit (CF request/response limits vs Fastly limits) and MUST NOT silently truncate.

## See also

- [D2 — fully buffered `Bytes` bodies in v1](../decisions/d02.md)
- [D21 — streaming response bodies (M6)](../decisions/d21.md)
- KV value handling: `api/kv` *(planned)*
