//! Compile-fail tests for `#[edge_core::main]` (SPEC §6.2 feature matrix).
//!
//! * default run — the "neither feature" error;
//! * `cargo test -p edge-macros --features fastly,cloudflare` — the
//!   "mutually exclusive" error (both features enabled).
//!
//! The `fastly`-positive and `cloudflare`-positive paths are exercised
//! end-to-end by `examples/hello-world` and `tests/conformance`;
//! signature-validation failures are unit-tested in `src/lib.rs`.
//!
//! Note: the `both_features` fixture's stderr also contains a harmless
//! "edge.toml not found" error — the trybuild project directory has no
//! `edge.toml` for the fastly branch's `include_str!` (SPEC D9).

/// Neither platform feature: clear compile error.
#[cfg(not(all(feature = "fastly", feature = "cloudflare")))]
#[test]
fn ui_neither_feature() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/neither_feature.rs");
}

/// Both platform features: mutually-exclusive compile error.
#[cfg(all(feature = "fastly", feature = "cloudflare"))]
#[test]
fn ui_both_features() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/both_features.rs");
}
