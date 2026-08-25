//! HTTP conversions between `edge_core` types and the workers-rs SDK
//! (SPEC §8.1): use the worker crate's `http`-feature helpers
//! (`request_from_wasm` / `response_to_wasm` / …), normalizing bodies via
//! full buffering by default (decision D2). Streaming response bodies (SPEC
//! D21) keep the `ReadableStream` live: `WorkerChunkStream` adapts
//! `worker::Body` to the core `ChunkStream` trait, and `EdgeBodyStream`
//! adapts the reverse direction for `Response::from_stream`.

use std::pin::Pin;
use std::task::{Context as TaskContext, Poll};

use bytes::Bytes;
use edge_core::{
    log::CONTROL_HEADER, types::ChunkStream, Body, Context, EdgeRequest, EdgeResponse, Error,
    LogLevel, Result as CoreResult,
};
use futures_util::Stream;
use http_body_util::BodyExt;
use wasm_bindgen::JsCast;

/// Convert a `web_sys::Request` to an [`EdgeRequest`], buffering the body.
pub async fn request_to_edge(req: web_sys::Request) -> CoreResult<EdgeRequest> {
    let http_req = worker::request_from_wasm(req).map_err(convert_err)?;
    let (parts, body) = http_req.into_parts();
    let bytes = collect(body).await?;
    Ok(http::Request::from_parts(parts, Body::buffered(bytes)))
}

/// Convert an [`EdgeResponse`] to a `web_sys::Response`.
///
/// Buffered bodies become a one-shot stream (empty → null body, D17);
/// streaming bodies become a live `ReadableStream` via `from_stream` (SPEC
/// D21), so large payloads stream to the client instead of being buffered.
///
/// Structured log fields ride in the control response header: any
/// origin/handler-supplied value is stripped first (P10) and the adapter's
/// finalized serialization is inserted (SPEC-PORTABILITY-PRIMITIVES §6).
pub async fn response_from_edge(
    resp: EdgeResponse,
    ctx: &Context,
) -> CoreResult<web_sys::Response> {
    let (parts, body) = resp.into_parts();
    let worker_body = match body {
        Body::Buffered(bytes) => {
            body_from_bytes(bytes).map_err(|e| Error::Internal(e.to_string()))?
        }
        Body::Streaming(_) => worker::Body::from_stream(EdgeBodyStream(body))
            .map_err(|e| Error::Internal(e.to_string()))?,
    };
    let http_resp: http::Response<worker::Body> = http::Response::from_parts(parts, worker_body);
    let ws_resp: web_sys::Response = worker::IntoResponse::into_raw(http_resp)
        .map_err(|e| Error::Internal(e.into().to_string()))?;
    Ok(apply_control_header(ws_resp, ctx))
}

/// Strip an origin/handler-supplied logging control header from a
/// `web_sys::Response` (P10) and insert the adapter's finalized serialized
/// fields when any are set (SPEC-PORTABILITY-PRIMITIVES §6).
///
/// The header is the boundary record on Cloudflare: the conformance harness
/// reads it as the finalization record. Origin values never reach it — they
/// are stripped and a diagnostic is emitted.
pub fn apply_control_header(resp: web_sys::Response, ctx: &Context) -> web_sys::Response {
    let headers = resp.headers();
    if headers.get(CONTROL_HEADER).ok().flatten().is_some() {
        let _ = headers.delete(CONTROL_HEADER);
        ctx.log(
            LogLevel::Warn,
            "stripped client-visible logging control header from the response \
             (origin-supplied value ignored)",
        );
    }
    if let Some(value) = ctx.finalize_log_fields() {
        let _ = headers.set(CONTROL_HEADER, &value);
    }
    resp
}

/// Bridge the core [`ChunkStream`] into the `futures::Stream` shape
/// `worker::Body::from_stream` consumes.
pub struct EdgeBodyStream(pub Body);

impl std::fmt::Debug for EdgeBodyStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("EdgeBodyStream")
            .field(&self.0.is_streaming())
            .finish()
    }
}

impl Stream for EdgeBodyStream {
    type Item = std::result::Result<Vec<u8>, worker::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<Option<Self::Item>> {
        match self.0.poll_next_chunk(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(Some(chunk))) => Poll::Ready(Some(Ok(chunk.to_vec()))),
            Poll::Ready(Ok(None)) => Poll::Ready(None),
            Poll::Ready(Err(e)) => Poll::Ready(Some(Err(worker::Error::RustError(e.to_string())))),
        }
    }
}

/// Convert a `web_sys::Response` to an [`EdgeResponse`], buffering the body.
pub async fn response_to_edge(resp: web_sys::Response) -> CoreResult<EdgeResponse> {
    let http_resp = worker::response_from_wasm(resp).map_err(convert_err)?;
    let (parts, body) = http_resp.into_parts();
    let bytes = collect(body).await?;
    Ok(http::Response::from_parts(parts, Body::buffered(bytes)))
}

/// Convert a `web_sys::Response` to an [`EdgeResponse`], keeping the body
/// as a stream (SPEC D21).
///
/// Headers are available immediately; the `ReadableStream` is wrapped in
/// [`WorkerChunkStream`] and read incrementally by the handler or relayed
/// to the client.
pub fn response_to_edge_streaming(resp: web_sys::Response) -> CoreResult<EdgeResponse> {
    let http_resp = worker::response_from_wasm(resp).map_err(convert_err)?;
    let (parts, body) = http_resp.into_parts();
    Ok(http::Response::from_parts(
        parts,
        Body::stream(WorkerChunkStream { body }),
    ))
}

/// A [`ChunkStream`] over a `worker::Body` (a `ReadableStream`).
///
/// Polling delegates to the underlying stream's `poll_next`, driven by the
/// JS event loop — genuinely async, so `Pending` is possible here (unlike
/// the Fastly adapter, SPEC D21).
#[derive(Debug)]
pub struct WorkerChunkStream {
    body: worker::Body,
}

impl ChunkStream for WorkerChunkStream {
    fn poll_next_chunk(&mut self, cx: &mut TaskContext<'_>) -> Poll<CoreResult<Option<Bytes>>> {
        match Pin::new(&mut self.body).poll_next(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(None) => Poll::Ready(Ok(None)),
            Poll::Ready(Some(Ok(bytes))) => Poll::Ready(Ok(Some(bytes))),
            Poll::Ready(Some(Err(e))) => {
                Poll::Ready(Err(Error::Body(std::io::Error::other(e.to_string()))))
            }
        }
    }
}

/// Buffer a `worker::Body` (a `ReadableStream`) to completion (SPEC §6.4).
pub async fn collect(body: worker::Body) -> CoreResult<Bytes> {
    let collected = BodyExt::collect(body)
        .await
        .map_err(|e| Error::Body(std::io::Error::other(e.to_string())))?;
    Ok(collected.to_bytes())
}

/// Build a `worker::Body` from buffered bytes (a one-shot stream).
///
/// Empty input becomes [`worker::Body::empty`] (a null body): web_sys
/// rejects requests whose body is present-but-empty for GET/HEAD (verified
/// under workerd, T4), so empty payloads must not materialize a stream.
pub fn body_from_bytes(bytes: Bytes) -> std::result::Result<worker::Body, worker::Error> {
    if bytes.is_empty() {
        return Ok(worker::Body::empty());
    }
    let stream =
        futures_util::stream::once(async move { Ok::<Vec<u8>, worker::Error>(bytes.to_vec()) });
    worker::Body::from_stream(stream)
}

/// Convert an [`EdgeRequest`] (already buffered `Bytes`) into a
/// promise-returning fetch call, keeping `redirect: manual` parity (D5.2).
///
/// The `web_sys::Request` is built directly from the public `RequestInit`
/// API rather than via `worker::request_to_wasm`, because workers-rs 0.8.5
/// only honors a `RequestRedirect` extension of its *private*
/// `http::redirect` type — `worker::RequestRedirect` (the public re-export
/// from `request_init`) is a different enum, so the extension is silently
/// dropped and the fetch would default to `follow` (verified under workerd:
/// a 302 from an ExternalServer then rejects with "Network connection lost").
///
/// Returns the raw promise; rejection (network failure) is handled by the
/// caller via `JsFuture`.
pub fn fetch_request_manual(http_req: EdgeRequest) -> CoreResult<js_sys::Promise> {
    let (parts, body) = http_req.into_parts();

    let init = web_sys::RequestInit::new();
    init.set_method(parts.method.as_str());

    let headers = web_sys::Headers::new().map_err(js_err)?;
    for (name, value) in parts.headers.iter() {
        let value = value
            .to_str()
            .map_err(|e| Error::Internal(format!("request header not ASCII: {e}")))?;
        headers.append(name.as_str(), value).map_err(js_err)?;
    }
    init.set_headers(&headers);
    init.set_redirect(web_sys::RequestRedirect::Manual);

    // Empty payloads get no body at all (D17: a null body is the wire-level
    // parity for GET/HEAD; web_sys rejects present-but-empty streams).
    // Request bodies are always buffered in v1 (SPEC D2).
    let bytes = body
        .as_bytes()
        .expect("request bodies must be buffered (SPEC D2)");
    if !bytes.is_empty() {
        init.set_body(&js_sys::Uint8Array::from(bytes));
    }

    let ws_req =
        web_sys::Request::new_with_str_and_init(&parts.uri.to_string(), &init).map_err(js_err)?;
    let scope: web_sys::WorkerGlobalScope = js_sys::global().unchecked_into();
    Ok(scope.fetch_with_request(&ws_req))
}

/// Map a JS-level error to [`Error::Internal`].
fn js_err(v: wasm_bindgen::JsValue) -> Error {
    Error::Internal(format!("request construction failed: {}", js_string(&v)))
}

fn convert_err(e: worker::Error) -> Error {
    Error::Internal(format!("request conversion failed: {e}"))
}

/// Render a JS error value as a string.
pub(crate) fn js_string(v: &wasm_bindgen::JsValue) -> String {
    v.as_string()
        .unwrap_or_else(|| "<non-string JS error>".to_string())
}
