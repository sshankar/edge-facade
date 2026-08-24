//! HTTP type conversions between `edge_core` types and the Fastly SDK
//! (SPEC §8.2). Bodies are fully buffered at this boundary (decision D2).

use std::io::Read;

use bytes::Bytes;
use edge_core::{error::PathError, EdgeRequest, EdgeResponse, Error, Result, StatusCode};

/// Read the client request, converting it to an [`EdgeRequest`].
///
/// # Panics
///
/// Panics if there is no client request in this session (Fastly runs one
/// instance per request; `Request::from_client` panics like `fastly::main`).
pub fn request_from_client() -> EdgeRequest {
    let req = fastly::Request::from_client();
    to_edge(req)
}

/// Convert a `fastly::Request` to an [`EdgeRequest`], buffering the body.
pub fn to_edge(req: fastly::Request) -> EdgeRequest {
    let http_req: http::Request<fastly::Body> = req.into();
    let (parts, body) = http_req.into_parts();
    let bytes = buffer(body).expect("buffering the client request body failed");
    http::Request::from_parts(parts, bytes)
}

/// Convert an [`EdgeRequest`] to a `fastly::Request`.
pub fn to_fastly(req: EdgeRequest) -> fastly::Request {
    let (parts, body) = req.into_parts();
    let fastly_body = fastly::Body::from(Vec::from(body));
    fastly::Request::from(http::Request::from_parts(parts, fastly_body))
}

/// Convert a `fastly::Response` to an [`EdgeResponse`], buffering the body.
pub fn from_fastly(resp: fastly::Response) -> Result<EdgeResponse> {
    let http_resp: http::Response<fastly::Body> = resp.into();
    let (parts, body) = http_resp.into_parts();
    let bytes = buffer(body)?;
    Ok(http::Response::from_parts(parts, bytes))
}

/// Send an [`EdgeResponse`] to the client.
///
/// # Panics
///
/// Panics if the host refuses the send (same convention as `send_to_client`).
pub fn response_to_client(resp: EdgeResponse) {
    let (parts, body) = resp.into_parts();
    let fastly_body = fastly::Body::from(Vec::from(body));
    let fastly_resp = fastly::Response::from(http::Response::from_parts(parts, fastly_body));
    fastly_resp.send_to_client();
}

/// Convert a handler error into a client response (SPEC §6.2).
///
/// Router misses become `404 Not Found` (PLAN-M0 §2); everything else is a
/// `500 Internal Server Error` with the error string, mirroring
/// `fastly::main`'s error convention.
pub fn error_to_client(e: &Error) {
    let status = match e {
        Error::Router(PathError::NotFound) => StatusCode::NOT_FOUND,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    let mut resp = fastly::Response::from_body(e.to_string());
    resp.set_status(status);
    resp.send_to_client();
}

/// Read a `fastly::Body` to the end, surfacing failures as [`Error::Body`]
/// (SPEC §6.4).
fn buffer(mut body: fastly::Body) -> Result<Bytes> {
    let mut buf = Vec::new();
    body.read_to_end(&mut buf).map_err(Error::from)?;
    Ok(Bytes::from(buf))
}
