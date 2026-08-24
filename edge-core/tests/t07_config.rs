//! T7: Config — vars and secrets from the mock platform.

use bytes::Bytes;
use edge_core::router::{handler, RouteParams};
use edge_core::testing::MockContextBuilder;
use edge_core::{Context, EdgeRequest, EdgeResponse, ResponseExt, Result, Router};

async fn read_config(
    _req: EdgeRequest,
    _params: RouteParams,
    ctx: Context,
) -> Result<EdgeResponse> {
    let foo_val = ctx.var("FOO").unwrap_or_else(|| "<missing>".to_string());
    let empty = ctx
        .var("EMPTY")
        .map(|s| format!("[{s}]"))
        .unwrap_or_else(|| "[missing]".to_string());
    let pass = ctx
        .secret("PASS")
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default();
    let missing = ctx.var("NOPE").is_none();
    Ok(EdgeResponse::ok(format!(
        "foo={foo_val};empty={empty};pass={pass};missing={missing}"
    )))
}

#[tokio::test]
async fn configured_keys_returned_unknown_none() {
    let mut router = Router::new();
    router.get("/config", handler(read_config)).unwrap();

    let mock = MockContextBuilder::new()
        .var("FOO", "bar")
        .var("EMPTY", "")
        .secret("PASS", "s3cret")
        .build();
    let mut ctx = mock.context();

    let req = http::Request::builder()
        .method("GET")
        .uri("/config")
        .body(edge_core::Body::from(Bytes::new()))
        .unwrap();
    let resp = router.handle(req, &mut ctx).await.unwrap();

    assert_eq!(
        resp.text().unwrap(),
        "foo=bar;empty=[];pass=s3cret;missing=true"
    );
}
