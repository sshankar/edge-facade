//! T1: Echo — method, path, query, headers, body round-trip through the router.

use bytes::Bytes;
use edge_core::router::{handler, RouteParams};
use edge_core::testing::MockContextBuilder;
use edge_core::{Context, EdgeRequest, EdgeResponse, ResponseExt, Result, Router, StatusCode};

async fn echo(req: EdgeRequest, _params: RouteParams, _ctx: Context) -> Result<EdgeResponse> {
    let method = req.method().to_string();
    let uri = req.uri().to_string();
    let mut resp = EdgeResponse::ok(req.into_body());
    resp.headers_mut()
        .insert("x-echo-method", method.parse().unwrap());
    resp.headers_mut()
        .insert("x-echo-uri", uri.parse().unwrap());
    Ok(resp)
}

#[tokio::test]
async fn t1_echo_round_trip() {
    let mut router = Router::new();
    // Any-method route: T1 verifies method/path/query/body passthrough.
    router.route("/echo", handler(echo)).unwrap();

    let mock = MockContextBuilder::new().build();
    let mut ctx = mock.context();

    let req = http::Request::builder()
        .method("POST")
        .uri("/echo?q=rust&lang=wasm")
        .header("x-in", "42")
        .body(edge_core::Body::from(Bytes::from("hello 世界")))
        .unwrap();

    let resp = router.handle(req, &mut ctx).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.text().unwrap(), "hello 世界");
    assert_eq!(resp.headers()["x-echo-method"], "POST");
    assert_eq!(resp.headers()["x-echo-uri"], "/echo?q=rust&lang=wasm");
}
