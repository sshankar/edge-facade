//! T4 (mock): `Context::fetch` — handler routing, unresolved hosts, faults.
//!
//! This pre-validates the fetch plumbing; real origin resolution is M3.

use bytes::Bytes;
use edge_core::testing::MockContextBuilder;
use edge_core::{EdgeResponse, Error, FetchError, ResponseExt};

#[tokio::test]
async fn fetch_routes_to_installed_handler() {
    let mock = MockContextBuilder::new()
        .on_fetch(|req| Ok(EdgeResponse::ok(format!("origin saw {}", req.uri()))))
        .build();
    let mut ctx = mock.context();

    let req = http::Request::builder()
        .method("GET")
        .uri("https://upstream.example/a?b=1")
        .body(edge_core::Body::from(Bytes::new()))
        .unwrap();

    let resp = ctx.fetch(req).await.unwrap();
    assert_eq!(
        resp.text().unwrap(),
        "origin saw https://upstream.example/a?b=1"
    );
    assert_eq!(mock.records().fetches.len(), 1);
}

#[tokio::test]
async fn fetch_without_handler_is_unresolved() {
    let mock = MockContextBuilder::new().build();
    let mut ctx = mock.context();

    let req = http::Request::builder()
        .uri("https://unknown.example/")
        .body(edge_core::Body::from(Bytes::new()))
        .unwrap();

    let err = ctx.fetch(req).await.unwrap_err();
    assert!(
        matches!(err, Error::Fetch(FetchError::UnresolvedBackend(host)) if host == "unknown.example")
    );
}

#[tokio::test]
async fn fetch_fault_injection() {
    let mock = MockContextBuilder::new().fail_fetch().build();
    let mut ctx = mock.context();

    let req = http::Request::builder()
        .uri("https://any.example/")
        .body(edge_core::Body::from(Bytes::new()))
        .unwrap();

    let err = ctx.fetch(req).await.unwrap_err();
    assert!(matches!(err, Error::Fetch(FetchError::Connection(_))));
}
