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
    let json: serde_json::Value =
        serde_json::from_slice(resp.body().as_bytes().expect("buffered body")).unwrap();
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
    assert_eq!(
        resp.body().as_bytes(),
        Some(&b"h\xc3\xa9llo \xe4\xb8\x96\xe7\x95\x8c"[..])
    );
}

#[tokio::test]
async fn t3_router_params_and_404() {
    let resp = call("/t3/hello/alice?q=hi").await.unwrap();
    let json: serde_json::Value =
        serde_json::from_slice(resp.body().as_bytes().expect("buffered body")).unwrap();
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
    let json: serde_json::Value =
        serde_json::from_slice(resp.body().as_bytes().expect("buffered body")).unwrap();
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
    let json: serde_json::Value =
        serde_json::from_slice(resp.body().as_bytes().expect("buffered body")).unwrap();
    assert_eq!(json["outcome"], "error");
    assert_eq!(json["category"], "UnresolvedBackend");
    assert_eq!(json["host"], "undeclared.example.com");
}

#[tokio::test]
async fn t5_undeclared_host_succeeds_with_handler() {
    // With a handler installed (mock_origin answers any host) the fetch
    // succeeds — the documented CF behavior (SPEC §7.5: fail open).
    let resp = call("/t5").await.unwrap();
    let json: serde_json::Value =
        serde_json::from_slice(resp.body().as_bytes().expect("buffered body")).unwrap();
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
    let json: serde_json::Value =
        serde_json::from_slice(resp.body().as_bytes().expect("buffered body")).unwrap();
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
    let json: serde_json::Value =
        serde_json::from_slice(resp.body().as_bytes().expect("buffered body")).unwrap();
    assert_eq!(json["greeting"], "Hello");
    assert_eq!(json["api_key"], "s3cret");
    assert_eq!(json["missing"], true);
}

#[tokio::test]
async fn t8_kv_round_trip_on_mock() {
    let resp = call("/t8").await.unwrap();
    let json: serde_json::Value =
        serde_json::from_slice(resp.body().as_bytes().expect("buffered body")).unwrap();
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
    let json: serde_json::Value =
        serde_json::from_slice(resp.body().as_bytes().expect("buffered body")).unwrap();
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

#[tokio::test]
async fn t12_streaming_relay_preserves_content() {
    // Mock origin serves /t12-origin as a multi-chunk stream; the handler
    // consumes exactly one chunk and relays the rest (SPEC §11 T12). The
    // relayed body must equal the origin payload minus the first chunk, and
    // the `x-t12-first-chunk` header must report that chunk's length.
    use bytes::Bytes;
    use edge_core::Body;

    let chunks = vec![
        Bytes::from_static(b"first-chunk-"),
        Bytes::from_static(b"second-chunk-"),
        Bytes::from_static(b"third"),
    ];
    let origin_chunks = chunks.clone();
    let mut ctx = MockContextBuilder::new()
        .on_fetch(move |req: EdgeRequest| {
            if req.uri().path() == "/t12-origin" {
                Ok(EdgeResponse::ok(Body::from_chunks(origin_chunks.clone())))
            } else {
                Ok(EdgeResponse::ok("unexpected path"))
            }
        })
        .build()
        .context();
    let router = edge_conformance::build_router().unwrap();
    let req = http::Request::builder()
        .uri("/t12")
        .body(Default::default())
        .unwrap();
    let resp = router.handle(req, &mut ctx).await.unwrap();

    let first: usize = resp.headers()["x-t12-first-chunk"]
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(first, chunks[0].len());

    let relay = resp.into_body().collect().await.unwrap();
    let expected: Bytes = chunks[1..].iter().flat_map(|c| c.iter().copied()).collect();
    assert_eq!(relay, expected);
}

#[tokio::test]
async fn t12_streaming_via_router_invariant_holds() {
    // Through the default JSON mock origin (buffered), fetch_streaming must
    // still present a streaming (one-shot) body; the T12 invariant
    // first-chunk + relayed body == full payload must hold (SPEC D21).
    let resp = call("/t12").await.unwrap();
    let first: usize = resp.headers()["x-t12-first-chunk"]
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    assert!(first > 0);

    let relay = resp.into_body().collect().await.unwrap();
    let full = r#"{"host":"api.example.com","path":"/t12-origin","query":""}"#;
    assert_eq!(relay.as_ref(), &full.as_bytes()[first..]);
}

// ===========================================================================
// M10/M11 — portability primitives: client metadata (P7/P8) and structured
// logging fields (P9–P11), SPEC-PORTABILITY-PRIMITIVES §11.
// ===========================================================================

use edge_core::client::{ClientMetadata, EdgeProvider, GeoMetadata, NetworkMetadata, TlsMetadata};

/// P7 — the mock reports exactly the fixture metadata; unavailable fields are
/// `None` (never substituted with presentation values).
#[tokio::test]
async fn p7_client_metadata_fixture() {
    let fixture = ClientMetadata {
        provider: EdgeProvider::Mock,
        client_ip: Some("203.0.113.7".parse().unwrap()),
        pop: Some("DFW".into()),
        original_header_names: Some(vec!["X-Mixed-Case".into(), "user-agent".into()]),
        geo: GeoMetadata {
            continent: Some("NA".into()),
            country_code: Some("US".into()),
            region_code: Some("TX".into()),
            city: Some("Austin".into()),
            postal_code: Some("78701".into()),
            metro_code: Some("635".into()),
            latitude: Some(30.27),
            longitude: Some(-97.74),
        },
        network: NetworkMetadata {
            asn: Some(64512),
            as_organization: Some("Example Org".into()),
            proxy_type: Some("Hosting".into()),
            proxy_description: Some("Cloud".into()),
        },
        tls: TlsMetadata {
            protocol: Some("TLSv1.3".into()),
            cipher: Some("AEAD-AES128-GCM-SHA256".into()),
            ja3: None,
            ja4: None,
            ciphers_sha1: None,
            extensions_sha1: None,
        },
    };
    let mock = MockContextBuilder::new().client_metadata(fixture).build();
    let router = edge_conformance::build_router().unwrap();
    let req = http::Request::builder()
        .uri("/p7")
        .body(Default::default())
        .unwrap();
    let mut ctx = mock.context();
    let resp = router.handle(req, &mut ctx).await.unwrap();
    let json: serde_json::Value =
        serde_json::from_slice(resp.body().as_bytes().expect("buffered body")).unwrap();
    assert_eq!(json["provider"], "Mock");
    assert_eq!(json["client_ip"], "203.0.113.7");
    assert_eq!(json["pop"], "DFW");
    assert_eq!(json["original_header_names"][0], "X-Mixed-Case");
    assert_eq!(json["geo"]["city"], "Austin");
    assert_eq!(json["geo"]["latitude"], 30.27);
    assert_eq!(json["network"]["asn"], 64512);
    assert_eq!(json["tls"]["cipher"], "AEAD-AES128-GCM-SHA256");
}

/// P7 (None leg) — the default mock reports every field as `None`, never a
/// presentation substitute.
#[tokio::test]
async fn p7_client_metadata_defaults_to_none() {
    let router = edge_conformance::build_router().unwrap();
    let mut ctx = MockContextBuilder::new().build().context();
    let req = http::Request::builder()
        .uri("/p7")
        .body(Default::default())
        .unwrap();
    let resp = router.handle(req, &mut ctx).await.unwrap();
    let json: serde_json::Value =
        serde_json::from_slice(resp.body().as_bytes().expect("buffered body")).unwrap();
    assert_eq!(json["provider"], "Mock");
    assert!(json["client_ip"].is_null());
    assert!(json["pop"].is_null());
    assert!(json["original_header_names"].is_null());
    assert!(json["geo"]["country_code"].is_null());
    assert!(json["network"]["asn"].is_null());
    assert!(json["tls"]["ja4"].is_null());
}

/// P8 — original header names are `None` when the platform does not expose
/// them, even though the request carried headers — never reconstructed from
/// normalized headers.
#[tokio::test]
async fn p8_original_headers_none_never_reconstructed() {
    let router = edge_conformance::build_router().unwrap();
    let mut ctx = MockContextBuilder::new().build().context();
    let req = http::Request::builder()
        .uri("/p8")
        .header("X-Mixed-Case", "value")
        .header("user-agent", "test")
        .body(Default::default())
        .unwrap();
    let resp = router.handle(req, &mut ctx).await.unwrap();
    let json: serde_json::Value =
        serde_json::from_slice(resp.body().as_bytes().expect("buffered body")).unwrap();
    assert!(json["original_header_names"].is_null());
    assert!(json["header_count"].as_u64().unwrap() >= 2);
    assert!(json["saw_original_case"].is_null());
}

/// P8 (Some leg) — when the platform exposes originals, the injected
/// original spelling is preserved.
#[tokio::test]
async fn p8_original_headers_preserved_when_available() {
    let mock = MockContextBuilder::new()
        .client_metadata(ClientMetadata {
            provider: EdgeProvider::Mock,
            original_header_names: Some(vec!["X-Mixed-Case".into()]),
            ..ClientMetadata::default()
        })
        .build();
    let router = edge_conformance::build_router().unwrap();
    let req = http::Request::builder()
        .uri("/p8")
        .header("X-Mixed-Case", "value")
        .body(Default::default())
        .unwrap();
    let mut ctx = mock.context();
    let resp = router.handle(req, &mut ctx).await.unwrap();
    let json: serde_json::Value =
        serde_json::from_slice(resp.body().as_bytes().expect("buffered body")).unwrap();
    assert_eq!(json["original_header_names"][0], "X-Mixed-Case");
    assert_eq!(json["saw_original_case"], serde_json::json!(true));
}

/// P9 — the same logical field map is captured on success and on synthetic
/// error, and the finalized snapshot matches on both legs.
#[tokio::test]
async fn p9_same_logical_map_on_success_and_error() {
    async fn run(path: &str) -> (Vec<(String, String)>, Option<Vec<(String, String)>>) {
        let router = edge_conformance::build_router().unwrap();
        let mock = MockContextBuilder::new().build();
        let req = http::Request::builder()
            .uri(path)
            .body(Default::default())
            .unwrap();
        let mut ctx = mock.context();
        let _ = router.handle(req, &mut ctx).await; // error leg returns Err
        ctx.finalize_log_fields();
        let records = mock.records();
        (records.log_fields, records.finalized_log_fields)
    }

    let (ok_map, ok_final) = run("/p9").await;
    let (err_map, err_final) = run("/p9-error").await;
    let expected = vec![
        ("request_id".to_string(), "req-123".to_string()),
        ("origin".to_string(), "api-a".to_string()),
    ];
    assert_eq!(ok_map, expected);
    assert_eq!(err_map, expected);
    assert_eq!(ok_final.as_deref(), Some(expected.as_slice()));
    assert_eq!(err_final.as_deref(), Some(expected.as_slice()));
}

/// P10 — an origin-supplied control header is stripped at the adapter
/// boundary (with a diagnostic) and the facade's serialized fields take its
/// place; the injected value never reaches the finalized record.
#[tokio::test]
async fn p10_control_field_stripped_with_diagnostic() {
    let router = edge_conformance::build_router().unwrap();
    let mock = MockContextBuilder::new().build();
    let req = http::Request::builder()
        .uri("/p10")
        .body(Default::default())
        .unwrap();
    let mut ctx = mock.context();
    let mut resp = router.handle(req, &mut ctx).await.unwrap();

    // The handler's response carries the injected control header...
    assert!(resp.headers().contains_key("x-edge-log-fields"));

    // ...and the adapter boundary strips it, replacing it with the facade's
    // own serialized fields, with a diagnostic.
    let stripped = edge_core::log::strip_control_header(&mut resp, &ctx);
    assert!(stripped);
    assert!(!resp.headers().contains_key("x-edge-log-fields"));
    let control = ctx.finalize_log_fields();
    assert_eq!(control.as_deref(), Some(r#"{"tenant":"t1"}"#));

    let records = mock.records();
    assert!(records
        .logs
        .iter()
        .any(|(_, msg)| msg.contains("logging control header")));
    assert_eq!(
        records.finalized_log_fields.as_deref(),
        Some(&[("tenant".to_string(), "t1".to_string())][..])
    );
}

/// P11 — aggregate budget overflow keeps the newest fields deterministically
/// and emits a diagnostic; the finalized control value carries exactly the
/// retained set.
#[tokio::test]
async fn p11_budget_retains_newest_deterministically() {
    let router = edge_conformance::build_router().unwrap();
    let mock = MockContextBuilder::new().build();
    let req = http::Request::builder()
        .uri("/p11")
        .body(Default::default())
        .unwrap();
    let mut ctx = mock.context();
    let resp = router.handle(req, &mut ctx).await.unwrap();
    assert_eq!(resp.status(), 200);

    let records = mock.records();
    // 20 fields of 303 bytes > 4096 -> the 13 newest survive (f07..=f19).
    assert_eq!(records.log_fields.len(), 13);
    assert_eq!(records.log_fields.first().unwrap().0, "f07");
    assert_eq!(records.log_fields.last().unwrap().0, "f19");
    assert!(records
        .logs
        .iter()
        .any(|(_, msg)| msg.contains("aggregate budget")));

    let control = ctx.finalize_log_fields().unwrap();
    let json: serde_json::Value = serde_json::from_str(&control).unwrap();
    let keys: Vec<&str> = json
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        vec![
            "f07", "f08", "f09", "f10", "f11", "f12", "f13", "f14", "f15", "f16", "f17", "f18",
            "f19"
        ]
    );
    assert_eq!(json["f19"], "x".repeat(300));
}

/// M11 unit — invalid keys fail with `Error::LogField` and empty values are
/// omitted.
#[tokio::test]
async fn log_field_key_validation_and_empty_omission() {
    let mock = MockContextBuilder::new().build();
    let ctx = mock.context();
    assert!(matches!(
        ctx.set_log_field("has space", "v"),
        Err(edge_core::Error::LogField(_))
    ));
    assert!(ctx.set_log_field("", "v").is_err());
    ctx.set_log_field("k", "v").unwrap();
    ctx.set_log_field("k", "").unwrap(); // omitted: existing value kept
    assert_eq!(mock.records().log_fields, vec![("k".into(), "v".into())]);
    ctx.remove_log_field("k");
    assert!(mock.records().log_fields.is_empty());
}
