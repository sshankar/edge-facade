//! T9: Logging — the `edge_core::log` macros emit to the mock sink.

use bytes::Bytes;
use edge_core::log::{error, info, warn};
use edge_core::router::{handler, RouteParams};
use edge_core::testing::MockContextBuilder;
use edge_core::{Context, EdgeRequest, EdgeResponse, LogLevel, ResponseExt, Result, Router};

async fn log_all(_req: EdgeRequest, _params: RouteParams, ctx: Context) -> Result<EdgeResponse> {
    info!(ctx, "handled {}", 42);
    warn!(ctx, "warming");
    error!(ctx, "boomed");
    Ok(EdgeResponse::ok("done"))
}

#[tokio::test]
async fn macros_emit_to_mock_sink() {
    let mut router = Router::new();
    router.get("/log", handler(log_all)).unwrap();

    let mock = MockContextBuilder::new().build();
    let mut ctx = mock.context();

    let req = http::Request::builder()
        .method("GET")
        .uri("/log")
        .body(edge_core::Body::from(Bytes::new()))
        .unwrap();
    router.handle(req, &mut ctx).await.unwrap();

    assert_eq!(
        mock.records().logs,
        vec![
            (LogLevel::Info, "handled 42".to_string()),
            (LogLevel::Warn, "warming".to_string()),
            (LogLevel::Error, "boomed".to_string()),
        ]
    );
}
