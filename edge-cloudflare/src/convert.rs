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
pub fn fetch_request_manual(http_req: http::Request<Bytes>) -> Result<js_sys::Promise> {
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
    if !body.is_empty() {
        init.set_body(&js_sys::Uint8Array::from(&body[..]));
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
