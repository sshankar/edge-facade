//! Fastly driver for the conformance suite (SPEC §11): builds the shared
//! scenario router and runs it as a Compute service under Viceroy.
//!
//! Build and run:
//!
//! ```text
//! cargo build -p conformance --features fastly --target wasm32-wasip1
//! viceroy serve -C fastly.toml \
//!     target/wasm32-wasip1/debug/conformance-fastly.wasm
//! ```

use edge_core::{Context, EdgeRequest, EdgeResponse, Result};

#[edge_core::main]
async fn main(req: EdgeRequest, mut ctx: Context) -> Result<EdgeResponse> {
    let router = edge_conformance::build_router()?;
    router.handle(req, &mut ctx).await
}
