//! T3: Router — path params, query extraction, 404s, method routing.

use bytes::Bytes;
use edge_core::router::{handler, RouteParams};
use edge_core::testing::MockContextBuilder;
use edge_core::{
    Context, EdgeRequest, EdgeResponse, Error, PathError, ResponseExt, Result, Router,
};

async fn greet(req: EdgeRequest, params: RouteParams, _ctx: Context) -> Result<EdgeResponse> {
    let name = params.get("name").unwrap_or("world");
    let query = req.uri().query().unwrap_or("");
    Ok(EdgeResponse::ok(format!("hello {name}?{query}")))
}

async fn anything(_req: EdgeRequest, _params: RouteParams, _ctx: Context) -> Result<EdgeResponse> {
    Ok(EdgeResponse::ok("any method"))
}

fn get_router() -> Router {
    let mut router = Router::new();
    router.get("/hello/:name", handler(greet)).unwrap();
    router.route("/anything", handler(anything)).unwrap();
    router
}

#[tokio::test]
async fn path_params_and_query_extraction() {
    let router = get_router();
    let mut ctx = MockContextBuilder::new().build().context();

    let req = http::Request::builder()
        .method("GET")
        .uri("/hello/alice?x=1")
        .body(Bytes::new())
        .unwrap();

    let resp = router.handle(req, &mut ctx).await.unwrap();
    assert_eq!(resp.text().unwrap(), "hello alice?x=1");
}

#[tokio::test]
async fn unknown_path_is_404() {
    let router = get_router();
    let mut ctx = MockContextBuilder::new().build().context();

    let req = http::Request::builder()
        .method("GET")
        .uri("/nope")
        .body(Bytes::new())
        .unwrap();
    let err = router.handle(req, &mut ctx).await.unwrap_err();
    assert!(matches!(err, Error::Router(PathError::NotFound)));
}

#[tokio::test]
async fn method_mismatch_is_404() {
    let router = get_router();
    let mut ctx = MockContextBuilder::new().build().context();

    let req = http::Request::builder()
        .method("POST")
        .uri("/hello/alice")
        .body(Bytes::new())
        .unwrap();
    let err = router.handle(req, &mut ctx).await.unwrap_err();
    assert!(matches!(err, Error::Router(PathError::NotFound)));
}

#[tokio::test]
async fn method_agnostic_route_matches_any_method() {
    let router = get_router();
    let mut ctx = MockContextBuilder::new().build().context();

    let req = http::Request::builder()
        .method("POST")
        .uri("/anything")
        .body(Bytes::new())
        .unwrap();
    let resp = router.handle(req, &mut ctx).await.unwrap();
    assert_eq!(resp.text().unwrap(), "any method");
}
