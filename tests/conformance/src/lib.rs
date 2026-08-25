//! Shared conformance scenarios (SPEC §11).
//!
//! Each scenario is a handler mounted on a [`Router`] by [`build_router`];
//! the same binary logic runs against the native mock context
//! (`tests/native.rs`) and, via `src/bin/fastly.rs`, under Viceroy. Every
//! test MUST behave identically on all three targets (host / Viceroy /
//! workerd in M2).

use edge_core::router::{handler, RouteParams};
use edge_core::{
    Body, Context, EdgeRequest, EdgeResponse, Error, FetchError, ResponseExt, Result, Router,
    StatusCode,
};
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
    let body = String::from_utf8_lossy(
        req.body()
            .as_bytes()
            .expect("request bodies are buffered (SPEC D2)"),
    )
    .into_owned();
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
    let body = String::from_utf8_lossy(
        resp.body()
            .as_bytes()
            .expect("fetch bodies are buffered (SPEC D2)"),
    )
    .into_owned();
    let mut out = EdgeResponse::with_status(resp.status(), body);
    // Expose the origin's status and the Host it saw, JSON-encoded by the
    // origin; passthrough status is asserted by the driver.
    out.headers_mut().insert(
        "x-origin-status",
        resp.status().as_u16().to_string().parse().unwrap(),
    );
    Ok(out)
}

/// T5 — fetch to an undeclared host (SPEC §11 T5): fail closed on Fastly
/// (`UnresolvedBackend`, D4), fail open on CF where any URL is fetchable.
/// The handler reports what happened; drivers assert each platform's
/// documented behavior (SPEC §7.5 — handlers MUST NOT assume both platforms
/// reach undeclared hosts).
async fn t5_undeclared(
    _req: EdgeRequest,
    _params: RouteParams,
    mut ctx: Context,
) -> Result<EdgeResponse> {
    #[derive(Serialize)]
    struct Report {
        outcome: String,
        category: String,
        host: String,
    }
    let fetch_req = http::Request::builder()
        .uri("http://undeclared.example.com/t5")
        .body(Default::default())
        .unwrap();
    let report = match ctx.fetch(fetch_req).await {
        Ok(_) => Report {
            outcome: "ok".into(),
            category: "-".into(),
            host: "undeclared.example.com".into(),
        },
        Err(Error::Fetch(e)) => Report {
            outcome: "error".into(),
            category: e.category().into(),
            host: match &e {
                FetchError::UnresolvedBackend(h) => h.clone(),
                _ => "?".into(),
            },
        },
        Err(_) => Report {
            outcome: "error".into(),
            category: "other".into(),
            host: "-".into(),
        },
    };
    let mut resp = EdgeResponse::ok("");
    resp.json(&report)?;
    Ok(resp)
}

/// T6 — fetch error surface (SPEC §11 T6): a refused origin must land in
/// the same `FetchError` category on both platforms. Fastly maps
/// `SendErrorCause::ConnectionRefused` to `Connection`; CF maps JS fetch
/// rejections to `Connection` too (D16: no typed causes in the CF runtime),
/// so the driver asserts `Connection` on both.
async fn t6_fetch_error(
    _req: EdgeRequest,
    _params: RouteParams,
    mut ctx: Context,
) -> Result<EdgeResponse> {
    #[derive(Serialize)]
    struct Report {
        outcome: String,
        category: String,
    }
    let fetch_req = http::Request::builder()
        .uri("http://refused.example.com:19999/t6")
        .body(Default::default())
        .unwrap();
    let report = match ctx.fetch(fetch_req).await {
        Ok(_) => Report {
            outcome: "ok".into(),
            category: "-".into(),
        },
        Err(Error::Fetch(e)) => Report {
            outcome: "error".into(),
            category: e.category().into(),
        },
        Err(_) => Report {
            outcome: "error".into(),
            category: "other".into(),
        },
    };
    let mut resp = EdgeResponse::ok("");
    resp.json(&report)?;
    Ok(resp)
}

/// Redirect parity (SPEC §7.4.2, D5.2) — not part of the T-series: the
/// origin's 302 is passed through unchanged on both platforms. Drivers
/// assert the 302 status and `Location` header survive — a platform that
/// auto-followed would return the target's 200 instead.
async fn r1_redirect(
    _req: EdgeRequest,
    _params: RouteParams,
    mut ctx: Context,
) -> Result<EdgeResponse> {
    let fetch_req = http::Request::builder()
        .uri("http://api.example.com/t7-redirect")
        .body(Default::default())
        .unwrap();
    ctx.fetch(fetch_req).await
}

/// T11 — two sequential fetches (SPEC §11 T11): sequential awaits are the
/// concurrency model Fastly's poll-loop executor supports (D3); the pair
/// must both succeed and be independent. Responses nest the two origin JSON
/// bodies so drivers can assert each hop's host/path.
async fn t11_sequential(
    _req: EdgeRequest,
    _params: RouteParams,
    mut ctx: Context,
) -> Result<EdgeResponse> {
    #[derive(Serialize)]
    struct Pair<'a> {
        first: &'a str,
        second: &'a str,
    }
    let first_req = http::Request::builder()
        .uri("http://api.example.com/t11-first")
        .body(Default::default())
        .unwrap();
    let first = ctx.fetch(first_req).await?;
    let second_req = http::Request::builder()
        .uri("http://api.example.com/t11-second")
        .body(Default::default())
        .unwrap();
    let second = ctx.fetch(second_req).await?;

    let first_body = String::from_utf8_lossy(
        first
            .body()
            .as_bytes()
            .expect("fetch bodies are buffered (SPEC D2)"),
    )
    .into_owned();
    let second_body = String::from_utf8_lossy(
        second
            .body()
            .as_bytes()
            .expect("fetch bodies are buffered (SPEC D2)"),
    )
    .into_owned();
    let pair = Pair {
        first: &first_body,
        second: &second_body,
    };
    let mut resp = EdgeResponse::ok("");
    resp.json(&pair)?;
    Ok(resp)
}

/// T7 — config: vars/secrets for configured keys; `None` otherwise
/// (SPEC §11 T7). The handler reports the resolved values; drivers assert
/// the configured ones are present and an unconfigured key is `None`.
async fn t7_config(_req: EdgeRequest, _params: RouteParams, ctx: Context) -> Result<EdgeResponse> {
    #[derive(Serialize)]
    struct Report {
        greeting: String,
        api_key: String,
        missing: bool,
    }
    let report = Report {
        greeting: ctx.var("GREETING").unwrap_or_default(),
        api_key: ctx
            .secret("API_KEY")
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default(),
        missing: ctx.var("NOPE").is_none(),
    };
    let mut resp = EdgeResponse::ok("");
    resp.json(&report)?;
    Ok(resp)
}

/// T8 — KV: put/get/delete round trip, binary values, and `get` of a
/// missing key → `None` (SPEC §11 T8). The handler reports each step's
/// outcome; drivers assert parity across platforms.
async fn t8_kv(_req: EdgeRequest, _params: RouteParams, ctx: Context) -> Result<EdgeResponse> {
    #[derive(Serialize)]
    struct Report {
        text: Option<String>,
        missing: bool,
        after_delete: bool,
        binary_ok: bool,
    }
    let kv = ctx.kv();

    kv.put("m4-key", "hello 世界").await?;
    let text = match kv.get("m4-key").await? {
        Some(v) => v.text().await?,
        None => None,
    };
    let missing = kv.get("m4-missing").await?.is_none();
    kv.delete("m4-key").await?;
    let after_delete = kv.get("m4-key").await?.is_none();

    // Binary round-trip (invalid UTF-8 stays intact through both stores).
    kv.put("m4-bin", Body::from_static(&[0xff, 0x00])).await?;
    let bin = match kv.get("m4-bin").await? {
        Some(v) => v.bytes().await?,
        None => bytes::Bytes::new(),
    };
    kv.delete("m4-bin").await?;

    let report = Report {
        text,
        missing,
        after_delete,
        binary_ok: bin.as_ref() == &[0xff, 0x00][..],
    };
    let mut resp = EdgeResponse::ok("");
    resp.json(&report)?;
    Ok(resp)
}

/// T12 — streaming fetch + relay (M6, SPEC §11 T12): `fetch_streaming`
/// returns the origin's headers immediately and its body as a streaming
/// source (never pre-buffered). The handler reads exactly one chunk —
/// incremental processing — then re-wraps the unread remainder as the
/// client response body, which is itself streamed to the client (SPEC D21).
///
/// Chunk boundaries are platform-dependent (Fastly read chunks vs CF
/// ReadableStream), so the handler reports the consumed chunk's length in
/// `x-t12-first-chunk` and drivers assert the invariant that holds on every
/// platform: first-chunk + relayed body == the origin's full payload.
async fn t12_streaming(
    _req: EdgeRequest,
    _params: RouteParams,
    mut ctx: Context,
) -> Result<EdgeResponse> {
    let fetch_req = http::Request::builder()
        .uri("http://api.example.com/t12-origin")
        .body(Default::default())
        .unwrap();
    let resp = ctx.fetch_streaming(fetch_req).await?;
    let status = resp.status();
    let mut body = resp.into_body();
    // Incremental processing: read exactly one chunk before relaying.
    let first = body.next_chunk().await?;
    let first_chunk_len = first.as_ref().map(|c| c.len()).unwrap_or(0);
    // Relay the unread remainder as a stream (Body: ChunkStream re-boxing).
    let mut out = EdgeResponse::with_status(status, Body::stream(body));
    out.headers_mut().insert(
        "x-t12-first-chunk",
        first_chunk_len.to_string().parse().unwrap(),
    );
    Ok(out)
}

/// P7 — client metadata fixture (SPEC-PORTABILITY-PRIMITIVES §11): the
/// handler reports the full [`Context::client`] snapshot; drivers assert the
/// available fields map and that unavailable fields are `None` (never
/// substituted with presentation values).
async fn p7_client_metadata(
    _req: EdgeRequest,
    _params: RouteParams,
    ctx: Context,
) -> Result<EdgeResponse> {
    let mut resp = EdgeResponse::ok("");
    resp.json(ctx.client())?;
    Ok(resp)
}

/// P8 — original header names (SPEC-PORTABILITY-PRIMITIVES §11): reports
/// `original_header_names` (the platform's answer — `None` on Cloudflare,
/// never reconstructed from normalized headers) plus the number of headers
/// the handler actually received, so drivers can prove a non-empty request
/// still reports `None` where the platform does not expose originals.
async fn p8_original_headers(
    req: EdgeRequest,
    _params: RouteParams,
    ctx: Context,
) -> Result<EdgeResponse> {
    #[derive(Serialize)]
    struct Report<'a> {
        original_header_names: &'a Option<Vec<String>>,
        header_count: usize,
        saw_original_case: Option<bool>,
    }
    // If originals are reported, they must include the original spelling of
    // the injected `X-Mixed-Case` header.
    let saw_original_case = ctx
        .client()
        .original_header_names
        .as_ref()
        .map(|names| names.iter().any(|n| n == "X-Mixed-Case"));
    let report = Report {
        original_header_names: &ctx.client().original_header_names,
        header_count: req.headers().len(),
        saw_original_case,
    };
    let mut resp = EdgeResponse::ok("");
    resp.json(&report)?;
    Ok(resp)
}

/// P9 — log fields on success (SPEC-PORTABILITY-PRIMITIVES §11): the same
/// logical map must be captured for a successful and a synthetic-error
/// response.
async fn p9_log_fields_success(
    _req: EdgeRequest,
    _params: RouteParams,
    ctx: Context,
) -> Result<EdgeResponse> {
    ctx.set_log_field("request_id", "req-123")?;
    ctx.set_log_field("origin", "api-a")?;
    Ok(EdgeResponse::ok("ok"))
}

/// P9 (error leg) — same fields, then a synthetic handler error. The
/// finalized map must match the success leg on every platform.
async fn p9_log_fields_error(
    _req: EdgeRequest,
    _params: RouteParams,
    ctx: Context,
) -> Result<EdgeResponse> {
    ctx.set_log_field("request_id", "req-123")?;
    ctx.set_log_field("origin", "api-a")?;
    Err(Error::Internal("synthetic failure (P9)".to_string()))
}

/// P10 — control-field injection (SPEC-PORTABILITY-PRIMITIVES §11): the
/// handler sets a log field AND injects an origin-supplied control header
/// into its response. The adapter must strip the injected value (emitting a
/// diagnostic) so the client never sees it, and the finalized fields still
/// reach the boundary record.
async fn p10_control_field_injection(
    _req: EdgeRequest,
    _params: RouteParams,
    ctx: Context,
) -> Result<EdgeResponse> {
    ctx.set_log_field("tenant", "t1")?;
    let mut resp = EdgeResponse::ok("ok");
    resp.headers_mut().insert(
        "x-edge-log-fields",
        "injected=origin-value".parse().unwrap(),
    );
    Ok(resp)
}

/// P11 — budget enforcement (SPEC-PORTABILITY-PRIMITIVES §11): 20 fields of
/// ~300 bytes exceed the 4096-byte aggregate budget; the retained set must
/// be deterministic — the 13 newest (`f07`..=`f19`) — and the diagnostics
/// emitted. Drivers assert the exact retained set (and on Fastly that no
/// control data reaches the client response).
async fn p11_log_field_budget(
    _req: EdgeRequest,
    _params: RouteParams,
    ctx: Context,
) -> Result<EdgeResponse> {
    for i in 0..20 {
        ctx.set_log_field(format!("f{i:02}"), "x".repeat(300))?;
    }
    Ok(EdgeResponse::ok("ok"))
}

/// Build the router with all scenarios mounted.
pub fn build_router() -> Result<Router> {
    let mut router = Router::new();
    router.get("/t1", handler(t1_echo))?;
    router.get("/t2", handler(t2_response))?;
    router.get("/t3/hello/:name", handler(t3_greet))?;
    router.get("/t4", handler(t4_fetch))?;
    router.get("/t5", handler(t5_undeclared))?;
    router.get("/t6", handler(t6_fetch_error))?;
    router.get("/t7", handler(t7_config))?;
    router.get("/t8", handler(t8_kv))?;
    router.get("/r1", handler(r1_redirect))?;
    router.get("/t11", handler(t11_sequential))?;
    router.get("/t12", handler(t12_streaming))?;
    router.get("/p7", handler(p7_client_metadata))?;
    router.get("/p8", handler(p8_original_headers))?;
    router.get("/p9", handler(p9_log_fields_success))?;
    router.get("/p9-error", handler(p9_log_fields_error))?;
    router.get("/p10", handler(p10_control_field_injection))?;
    router.get("/p11", handler(p11_log_field_budget))?;
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
