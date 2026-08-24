//! T2: Response construction — status, header passthrough, UTF-8 bodies.

use bytes::Bytes;
use edge_core::{EdgeResponse, Error, ResponseExt, StatusCode};

#[test]
fn status_and_header_passthrough() {
    let mut resp = EdgeResponse::with_status(StatusCode::CREATED, "made");
    resp.headers_mut()
        .insert("x-custom", "value".parse().unwrap());

    assert_eq!(resp.status(), StatusCode::CREATED);
    assert_eq!(resp.headers()["x-custom"], "value");
    assert_eq!(resp.body(), &b"made"[..]);
}

#[test]
fn utf8_body_round_trip() {
    let body = "héllo 世界 🎉";
    let resp = EdgeResponse::ok(Bytes::from(body));
    assert_eq!(resp.text().unwrap(), body);
}

#[test]
fn invalid_utf8_is_a_body_error() {
    let resp = EdgeResponse::ok(Bytes::from_static(&[0xff, 0xfe, 0xfd]));
    assert!(matches!(resp.text(), Err(Error::Body(_))));
}

#[test]
fn json_helper_sets_type_and_is_parseable() {
    let mut resp = EdgeResponse::ok(Bytes::new());
    resp.json(&serde_json::json!({ "ok": true, "n": 7 }))
        .unwrap();

    assert_eq!(resp.headers()["content-type"], "application/json");
    let parsed: serde_json::Value = serde_json::from_slice(resp.body()).unwrap();
    assert_eq!(parsed["ok"], true);
    assert_eq!(parsed["n"], 7);
}
