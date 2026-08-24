//! Shared conformance scenarios (SPEC §11).
//!
//! Each scenario is a handler mounted on a [`Router`] by [`build_router`];
//! the same binary logic runs against the native mock context
//! (`tests/native.rs`) and, via `src/bin/fastly.rs`, under Viceroy. Every
//! test MUST behave identically on all three targets (host / Viceroy /
//! workerd in M2).

use edge_core::router::{handler, RouteParams};
use edge_core::{Context, EdgeRequest, EdgeResponse, ResponseExt, Result, Router, StatusCode};
use serde::Serialize;

/// Response bodies are JSON so drivers can assert structurally.
///
/// T1 — echo: method, path, query, a request header, and body round-trip
/// identically (SPEC §11 T1).
async fn t1_echo(req: EdgeRequest, _params: RouteParams, _ctx: Context) -> Result<EdgeResponse> {
    #[derive(Serialize)]
    struct Echo<'a> {
        method: &'a str,
        path: &'a str,
        query: Option<&'a str>,
        header_x_test: Option<&'a str>,
        body: String,
    }
    let body = String::from_utf8_lossy(req.body()).into_owned();
    let echo = Echo {
        method: req.method().as_str(),
        path: req.uri().path(),
        query: req.uri().query(),
        header_x_test: req.headers().get("x-test").and_then(|v| v.to_str().ok()),
        body,
    };
    let mut resp = EdgeResponse::ok("");
    resp.json(&echo)?;
    Ok(resp)
}

/// T2 — status + header passthrough and a multi-byte UTF-8 body (SPEC §11
/// T2).
async fn t2_response(
    _req: EdgeRequest,
    _params: RouteParams,
    _ctx: Context,
) -> Result<EdgeResponse> {
    let mut resp = EdgeResponse::with_status(StatusCode::CREATED, "héllo 世界");
    resp.headers_mut()
        .insert("x-conformance", "yes".parse().unwrap());
    resp.headers_mut()
        .insert("content-type", "text/plain; charset=utf-8".parse().unwrap());
    Ok(resp)
}

/// T3 — router: path params and query extraction; unmatched routes 404
/// (SPEC §11 T3).
async fn t3_greet(req: EdgeRequest, params: RouteParams, _ctx: Context) -> Result<EdgeResponse> {
    #[derive(Serialize)]
    struct Greeting<'a> {
        name: &'a str,
        query_q: Option<&'a str>,
    }
    let greeting = Greeting {
        name: params.get("name").unwrap_or("?"),
        query_q: req.uri().query(),
    };
    let mut resp = EdgeResponse::ok("");
    resp.json(&greeting)?;
    Ok(resp)
}

/// T4 — fetch to a declared origin; the origin echoes the Host header it
/// received, so drivers assert Host == URL host (parity rule D5.1,
/// SPEC §11 T4).
async fn t4_fetch(
    _req: EdgeRequest,
    _params: RouteParams,
    mut ctx: Context,
) -> Result<EdgeResponse> {
    let origin_url = "http://api.example.com/t4-origin?from=t4";
    let fetch_req = http::Request::builder()
        .uri(origin_url)
        .body(Default::default())
        .unwrap();
    let resp = ctx.fetch(fetch_req).await?;
    let body = String::from_utf8_lossy(resp.body()).into_owned();
    let mut out = EdgeResponse::with_status(resp.status(), body);
    // Expose the origin's status and the Host it saw, JSON-encoded by the
    // origin; passthrough status is asserted by the driver.
    out.headers_mut().insert(
        "x-origin-status",
        resp.status().as_u16().to_string().parse().unwrap(),
    );
    Ok(out)
}

/// Build the router with all scenarios mounted.
pub fn build_router() -> Result<Router> {
    let mut router = Router::new();
    router.get("/t1", handler(t1_echo))?;
    router.get("/t2", handler(t2_response))?;
    router.get("/t3/hello/:name", handler(t3_greet))?;
    router.get("/t4", handler(t4_fetch))?;
    Ok(router)
}

/// Cloudflare entry: exported as `fetch` from the cdylib (worker-build),
/// running the same scenarios as the Fastly bin and the native driver.
#[cfg(feature = "cloudflare")]
mod entry {
    use super::*;

    #[edge_core::main]
    pub async fn edge_main(req: EdgeRequest, mut ctx: Context) -> Result<EdgeResponse> {
        crate::build_router()?.handle(req, &mut ctx).await
    }
}
