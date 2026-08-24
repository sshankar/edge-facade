//! Fastly entry for hello-world (built with `--features fastly`).
//!
//! The shared handler lives in the lib; this bin provides the wasm `_start`
//! entry via `#[edge_core::main]`.

use edge_core::{Context, EdgeRequest, EdgeResponse, Result};

#[edge_core::main]
async fn main(req: EdgeRequest, ctx: Context) -> Result<EdgeResponse> {
    hello_world::handle(req, ctx).await
}
