//! `#[edge_core::main]` with neither `fastly` nor `cloudflare` feature
//! enabled must fail with a clear compile error (SPEC §6.2).
//!
//! Real service crates declare the platform features, so the
//! `unexpected_cfgs` lint is silenced here.
#![allow(unexpected_cfgs)]

use edge_core::{Context, EdgeRequest, EdgeResponse, ResponseExt, Result};

#[edge_core::main]
async fn main(_req: EdgeRequest, _ctx: Context) -> Result<EdgeResponse> {
    Ok(EdgeResponse::ok("hi"))
}

fn main() {}
