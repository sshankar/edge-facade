//! `#[edge_core::main]` with BOTH `fastly` and `cloudflare` features
//! enabled must fail with the mutually-exclusive compile error (SPEC §6.2).
//!
//! Run the ui tests with both features to exercise this path:
//! `cargo test -p edge-macros --features fastly,cloudflare`.
#![allow(unexpected_cfgs)]

use edge_core::{Context, EdgeRequest, EdgeResponse, ResponseExt, Result};

#[edge_core::main]
async fn main(_req: EdgeRequest, _ctx: Context) -> Result<EdgeResponse> {
    Ok(EdgeResponse::ok("hi"))
}
