//! Common HTTP types and helpers.
//!
//! Bodies are fully buffered as [`bytes::Bytes`] in v1 (SPEC §6.1, decision D2).

use bytes::Bytes;
use serde::Serialize;

/// A fully-buffered request/response body.
pub type Body = Bytes;

/// A platform-independent request.
pub type EdgeRequest = http::Request<Body>;

/// A platform-independent response.
pub type EdgeResponse = http::Response<Body>;

pub use http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri, Version};
pub use url::Url;

/// Convenience constructors/accessors for [`EdgeResponse`] (i.e.
/// `http::Response<Bytes>`).
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
    /// Returns [`Error::Body`](crate::Error::Body) if the body is not valid UTF-8.
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
        std::str::from_utf8(self.body()).map_err(utf8_err)
    }
}

fn json_err(e: serde_json::Error) -> crate::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, e).into()
}

fn utf8_err(e: std::str::Utf8Error) -> crate::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, e).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Error;

    #[test]
    fn ok_defaults_to_200_with_empty_body() {
        let resp = EdgeResponse::ok(Bytes::new());
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(resp.body().is_empty());
    }

    #[test]
    fn status_sets_status_and_body() {
        let resp = EdgeResponse::with_status(StatusCode::NOT_FOUND, "nope");
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        assert_eq!(resp.body(), &b"nope"[..]);
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
        assert_eq!(resp.body(), &b"{\"n\":7}"[..]);
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
}
