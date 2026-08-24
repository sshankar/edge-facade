//! HTTP type conversions between `edge_core` types and the Fastly SDK
//! (SPEC §8.2). Bodies are fully buffered at this boundary by default
//! (decision D2); `from_fastly_streaming` and the streaming branch of
//! `response_to_client` implement streaming response bodies (SPEC D21).

use std::io::{Read, Write};
use std::task::{Context as TaskContext, Poll, Waker};

use bytes::Bytes;
use edge_core::{
    error::PathError, types::ChunkStream, Body, EdgeRequest, EdgeResponse, Error, Result,
    StatusCode,
};

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
    http::Request::from_parts(parts, Body::buffered(bytes))
}

/// Convert an [`EdgeRequest`] to a `fastly::Request`.
///
/// # Panics
///
/// Panics on a streaming request body: request bodies are buffered in v1
/// (SPEC D2; streaming requests are not part of the M6 scope).
pub fn to_fastly(req: EdgeRequest) -> fastly::Request {
    let (parts, body) = req.into_parts();
    let bytes = body
        .into_bytes()
        .expect("request bodies must be buffered (SPEC D2)");
    let fastly_body = fastly::Body::from(Vec::from(bytes));
    fastly::Request::from(http::Request::from_parts(parts, fastly_body))
}

/// Convert a `fastly::Response` to an [`EdgeResponse`], buffering the body.
pub fn from_fastly(resp: fastly::Response) -> Result<EdgeResponse> {
    let http_resp: http::Response<fastly::Body> = resp.into();
    let (parts, body) = http_resp.into_parts();
    let bytes = buffer(body)?;
    Ok(http::Response::from_parts(parts, Body::buffered(bytes)))
}

/// Convert a `fastly::Response` to an [`EdgeResponse`], keeping the body
/// as a stream (SPEC D21).
///
/// `send` returns once response headers are received; the `fastly::Body`
/// handle is kept live and read incrementally through [`FastlyChunkStream`].
pub fn from_fastly_streaming(resp: fastly::Response) -> Result<EdgeResponse> {
    let http_resp: http::Response<fastly::Body> = resp.into();
    let (parts, body) = http_resp.into_parts();
    Ok(http::Response::from_parts(
        parts,
        Body::stream(FastlyChunkStream { body }),
    ))
}

/// A [`ChunkStream`] over a `fastly::Body` handle.
///
/// Reads are blocking host calls, so `poll_next_chunk` always returns
/// `Ready` — compatible with the D3 poll-loop executor: no select-scheduler
/// is needed for sequential streaming (SPEC D21; concurrency across streams
/// is M7).
#[derive(Debug)]
struct FastlyChunkStream {
    body: fastly::Body,
}

impl ChunkStream for FastlyChunkStream {
    fn poll_next_chunk(&mut self, _cx: &mut TaskContext<'_>) -> Poll<Result<Option<Bytes>>> {
        let mut buf = [0u8; 16 * 1024];
        match self.body.read(&mut buf) {
            Ok(0) => Poll::Ready(Ok(None)),
            Ok(n) => Poll::Ready(Ok(Some(Bytes::copy_from_slice(&buf[..n])))),
            Err(e) => Poll::Ready(Err(Error::from(e))),
        }
    }
}

/// Send an [`EdgeResponse`] to the client.
///
/// Buffered bodies are sent whole (v1). Streaming bodies are sent with
/// `stream_to_client`: headers go out first, then chunks are written as they
/// are read — the client observes the response progressively (SPEC D21).
///
/// # Panics
///
/// Panics if the host refuses the send (same convention as `send_to_client`).
pub fn response_to_client(resp: EdgeResponse) {
    let (parts, body) = resp.into_parts();
    match body {
        Body::Buffered(bytes) => {
            let fastly_body = fastly::Body::from(Vec::from(bytes));
            let fastly_resp =
                fastly::Response::from(http::Response::from_parts(parts, fastly_body));
            fastly_resp.send_to_client();
        }
        Body::Streaming(mut stream) => {
            let fastly_resp =
                fastly::Response::from(http::Response::from_parts(parts, fastly::Body::new()));
            let mut client = fastly_resp.stream_to_client();
            let waker = Waker::noop();
            let mut cx = TaskContext::from_waker(waker);
            loop {
                match stream.poll_next_chunk(&mut cx) {
                    Poll::Ready(Ok(Some(chunk))) => {
                        if client.write_all(&chunk).is_err() {
                            let _ = client.abandon();
                            return;
                        }
                    }
                    Poll::Ready(Ok(None)) => {
                        let _ = client.finish();
                        return;
                    }
                    Poll::Ready(Err(_)) => {
                        let _ = client.abandon();
                        return;
                    }
                    // Fastly chunk reads are blocking host calls (SPEC D21):
                    // a streaming body never reports Pending.
                    Poll::Pending => unreachable!(
                        "edge-fastly: streaming body returned Pending; \
                         Fastly chunk reads are blocking (SPEC D21)"
                    ),
                }
            }
        }
    }
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
