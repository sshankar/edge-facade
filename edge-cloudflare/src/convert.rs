//! HTTP conversions between `edge_core` types and the workers-rs SDK
//! (SPEC §8.1): use the worker crate's `http`-feature helpers
//! (`request_from_wasm` / `response_to_wasm` / …), normalizing bodies via
//! full buffering (decision D2).

use bytes::Bytes;
use edge_core::{EdgeRequest, EdgeResponse, Error, Result};
use http_body_util::BodyExt;
use wasm_bindgen::JsCast;

/// Convert a `web_sys::Request` to an [`EdgeRequest`], buffering the body.
pub async fn request_to_edge(req: web_sys::Request) -> Result<EdgeRequest> {
    let http_req = worker::request_from_wasm(req).map_err(convert_err)?;
    let (parts, body) = http_req.into_parts();
    let bytes = collect(body).await?;
    Ok(http::Request::from_parts(parts, bytes))
}

/// Convert an [`EdgeResponse`] to a `web_sys::Response`.
pub async fn response_from_edge(resp: EdgeResponse) -> Result<web_sys::Response> {
    let (parts, body) = resp.into_parts();
    let worker_body = body_from_bytes(body).map_err(|e| Error::Internal(e.to_string()))?;
    let http_resp: http::Response<worker::Body> = http::Response::from_parts(parts, worker_body);
    worker::IntoResponse::into_raw(http_resp).map_err(|e| Error::Internal(e.into().to_string()))
}

/// Convert a `web_sys::Response` to an [`EdgeResponse`], buffering the body.
pub async fn response_to_edge(resp: web_sys::Response) -> Result<EdgeResponse> {
    let http_resp = worker::response_from_wasm(resp).map_err(convert_err)?;
    let (parts, body) = http_resp.into_parts();
    let bytes = collect(body).await?;
    Ok(http::Response::from_parts(parts, bytes))
}

/// Buffer a `worker::Body` (a `ReadableStream`) to completion (SPEC §6.4).
pub async fn collect(body: worker::Body) -> Result<Bytes> {
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

/// Convert a `web_sys::Request` into a promise-returning fetch call, keeping
/// `redirect: manual` parity (D5.2).
///
/// Returns the raw promise; rejection (network failure) is handled by the
/// caller via `JsFuture`.
pub fn fetch_request_manual(http_req: http::Request<worker::Body>) -> Result<js_sys::Promise> {
    let mut http_req = http_req;
    http_req
        .extensions_mut()
        .insert(worker::RequestRedirect::Manual);
    let ws_req = worker::request_to_wasm(http_req).map_err(convert_err)?;
    let scope: web_sys::WorkerGlobalScope = js_sys::global().unchecked_into();
    Ok(scope.fetch_with_request(&ws_req))
}

fn convert_err(e: worker::Error) -> Error {
    Error::Internal(format!("request conversion failed: {e}"))
}

/// Render a JS error value as a string.
pub(crate) fn js_string(v: &wasm_bindgen::JsValue) -> String {
    v.as_string()
        .unwrap_or_else(|| "<non-string JS error>".to_string())
}
