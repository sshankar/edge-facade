//! Native driver for the conformance suite (SPEC §11): the same scenarios
//! as the Viceroy run, exercised against the mock context on the host.

use edge_core::testing::MockContextBuilder;
use edge_core::{Context, EdgeRequest, EdgeResponse, ResponseExt, Result, StatusCode};

/// Mock origin: echoes the same JSON shape the real origin server produces
/// in the Viceroy run (host, path, query), so assertions are identical.
fn mock_origin(req: EdgeRequest) -> Result<EdgeResponse> {
    let host = req.uri().host().map(str::to_string).unwrap_or_default();
    let body = format!(
        r#"{{"host":"{host}","path":"{}","query":"{}"}}"#,
        req.uri().path(),
        req.uri().query().unwrap_or("")
    );
    Ok(EdgeResponse::ok(body))
}

fn mock_ctx() -> Context {
    MockContextBuilder::new()
        .var("GREETING", "Hello")
        .secret("API_KEY", b"s3cret".to_vec())
        .kv_entry("default", "k", "v")
        .on_fetch(mock_origin)
        .build()
        .context()
}

async fn call(path: &str) -> Result<EdgeResponse> {
    let router = edge_conformance::build_router()?;
    let mut ctx = mock_ctx();
    let req = http::Request::builder()
        .uri(path)
        .body(Default::default())
        .unwrap();
    router.handle(req, &mut ctx).await
}

#[tokio::test]
async fn t1_echo_round_trips() {
    let router = edge_conformance::build_router().unwrap();
    let mut ctx = mock_ctx();
    let req = http::Request::builder()
        .uri("/t1?q=1&q=2")
        .header("x-test", "conformance")
        .body(edge_core::Body::from("some body".to_string().into_bytes()))
        .unwrap();
    let resp = router.handle(req, &mut ctx).await.unwrap();
    assert_eq!(resp.status(), 200);
    let json: serde_json::Value = serde_json::from_slice(resp.body()).unwrap();
    assert_eq!(json["method"], "GET");
    assert_eq!(json["path"], "/t1");
    assert_eq!(json["query"], "q=1&q=2");
    assert_eq!(json["header_x_test"], "conformance");
    assert_eq!(json["body"], "some body");
}

#[tokio::test]
async fn t2_response_is_identical() {
    let resp = call("/t2").await.unwrap();
    assert_eq!(resp.status(), 201);
    assert_eq!(resp.headers()["x-conformance"], "yes");
    assert_eq!(resp.body(), &b"h\xc3\xa9llo \xe4\xb8\x96\xe7\x95\x8c"[..]);
}

#[tokio::test]
async fn t3_router_params_and_404() {
    let resp = call("/t3/hello/alice?q=hi").await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(resp.body()).unwrap();
    assert_eq!(json["name"], "alice");
    assert_eq!(json["query_q"], "q=hi");

    // Unmatched route → 404 (router miss converted by the platform driver).
    let router = edge_conformance::build_router().unwrap();
    let mut ctx = mock_ctx();
    let req = http::Request::builder()
        .uri("/t3/unknown-route")
        .body(Default::default())
        .unwrap();
    let err = router.handle(req, &mut ctx).await.unwrap_err();
    assert!(matches!(
        err,
        edge_core::Error::Router(edge_core::PathError::NotFound)
    ));
}

#[tokio::test]
async fn t4_fetch_reaches_declared_origin() {
    let resp = call("/t4").await.unwrap();
    assert_eq!(resp.status(), 200);
    let json: serde_json::Value = serde_json::from_slice(resp.body()).unwrap();
    // The mock origin sees the URL host; the real origin under Viceroy sees
    // the same via override_host (D5.1).
    assert_eq!(json["host"], "api.example.com");
    assert_eq!(json["path"], "/t4-origin");
}

#[tokio::test]
async fn t5_undeclared_host_fails_closed_without_handler() {
    // No fetch handler installed -> the mock behaves like Fastly's
    // fail-closed resolution (D4): the host is undeclared.
    let router = edge_conformance::build_router().unwrap();
    let mut ctx = MockContextBuilder::new().build().context();
    let req = http::Request::builder()
        .uri("/t5")
        .body(Default::default())
        .unwrap();
    let resp = router.handle(req, &mut ctx).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(resp.body()).unwrap();
    assert_eq!(json["outcome"], "error");
    assert_eq!(json["category"], "UnresolvedBackend");
    assert_eq!(json["host"], "undeclared.example.com");
}

#[tokio::test]
async fn t5_undeclared_host_succeeds_with_handler() {
    // With a handler installed (mock_origin answers any host) the fetch
    // succeeds — the documented CF behavior (SPEC §7.5: fail open).
    let resp = call("/t5").await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(resp.body()).unwrap();
    assert_eq!(json["outcome"], "ok");
}

#[tokio::test]
async fn t6_refused_origin_surfaces_connection_category() {
    // fail_fetch injects FetchError::Connection — the category both
    // platforms must report for a refused origin (D16 on CF, SendErrorCause
    // mapping on Fastly).
    let router = edge_conformance::build_router().unwrap();
    let mut ctx = MockContextBuilder::new().fail_fetch().build().context();
    let req = http::Request::builder()
        .uri("/t6")
        .body(Default::default())
        .unwrap();
    let resp = router.handle(req, &mut ctx).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(resp.body()).unwrap();
    assert_eq!(json["outcome"], "error");
    assert_eq!(json["category"], "Connection");
}

#[tokio::test]
async fn r1_redirect_is_not_followed() {
    // The mock origin redirects /t7-redirect -> /t7-target; the adapter must
    // pass the 302 through (D5.2), never follow it.
    let mut ctx = MockContextBuilder::new()
        .on_fetch(|req| {
            if req.uri().path() == "/t7-redirect" {
                let mut resp = EdgeResponse::with_status(StatusCode::FOUND, "");
                resp.headers_mut()
                    .insert("location", "/t7-target".parse().unwrap());
                Ok(resp)
            } else {
                Ok(EdgeResponse::ok("redirect target"))
            }
        })
        .build()
        .context();
    let router = edge_conformance::build_router().unwrap();
    let req = http::Request::builder()
        .uri("/r1")
        .body(Default::default())
        .unwrap();
    let resp = router.handle(req, &mut ctx).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FOUND);
    assert_eq!(resp.headers()["location"], "/t7-target");
}

#[tokio::test]
async fn t7_config_values_and_missing_none() {
    let router = edge_conformance::build_router().unwrap();
    let mut ctx = MockContextBuilder::new()
        .var("GREETING", "Hello")
        .secret("API_KEY", "s3cret")
        .build()
        .context();
    let req = http::Request::builder()
        .uri("/t7")
        .body(Default::default())
        .unwrap();
    let resp = router.handle(req, &mut ctx).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(resp.body()).unwrap();
    assert_eq!(json["greeting"], "Hello");
    assert_eq!(json["api_key"], "s3cret");
    assert_eq!(json["missing"], true);
}

#[tokio::test]
async fn t8_kv_round_trip_on_mock() {
    let resp = call("/t8").await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(resp.body()).unwrap();
    assert_eq!(json["text"], "hello 世界");
    assert_eq!(json["missing"], true);
    assert_eq!(json["after_delete"], true);
    assert_eq!(json["binary_ok"], true);
}

#[tokio::test]
async fn t11_sequential_fetches_both_succeed() {
    // Two awaited fetches in sequence (SPEC §11 T11): the Fastly poll-loop
    // executor must drive both to completion, and each hop must be
    // independent (distinct paths).
    let resp = call("/t11").await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(resp.body()).unwrap();
    let first: serde_json::Value = serde_json::from_str(json["first"].as_str().unwrap()).unwrap();
    let second: serde_json::Value = serde_json::from_str(json["second"].as_str().unwrap()).unwrap();
    assert_eq!(first["host"], "api.example.com");
    assert_eq!(first["path"], "/t11-first");
    assert_eq!(second["host"], "api.example.com");
    assert_eq!(second["path"], "/t11-second");
}

#[tokio::test]
async fn t4_fetch_missing_origin_fails_closed() {
    let router = edge_conformance::build_router().unwrap();
    let mut ctx = MockContextBuilder::new() // no on_fetch → unresolved
        .build()
        .context();
    let req = http::Request::builder()
        .uri("/t4")
        .body(Default::default())
        .unwrap();
    let err = router.handle(req, &mut ctx).await.unwrap_err();
    assert!(matches!(
        err,
        edge_core::Error::Fetch(edge_core::FetchError::UnresolvedBackend(h))
            if h == "api.example.com"
    ));
}
