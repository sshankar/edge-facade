//! # edge-core
//!
//! Platform-agnostic core for the Edge SDK: one Rust handler, deployed to both
//! Cloudflare Workers and Fastly Compute.
//!
//! This crate contains **no platform dependencies** (no wasm-bindgen, no
//! fastly-sys). It compiles on the host, on `wasm32-unknown-unknown`, and on
//! `wasm32-wasip1`/`wasm32-wasip2`. Platform adapters live in separate crates
//! (`edge-cloudflare`, `edge-fastly`) and implement the internal
//! [`Platform`](crate::context::Platform) SPI.
//!
//! # Example
//!
//! ```
//! use edge_core::{Context, EdgeRequest, EdgeResponse, ResponseExt, Result, Router};
//! use edge_core::router::{handler, RouteParams};
//!
//! async fn hello(_req: EdgeRequest, params: RouteParams, _ctx: Context) -> Result<EdgeResponse> {
//!     let name = params.get("name").unwrap_or("world");
//!     Ok(EdgeResponse::ok(format!("hello {name}")))
//! }
//!
//! # fn build() -> Result<Router> {
//! let mut router = Router::new();
//! router.get("/hello/:name", handler(hello))?;
//! Ok(router)
//! # }
//! ```

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![warn(missing_debug_implementations)]

pub mod config;
pub mod context;
pub mod error;
pub mod kv;
pub mod log;
pub mod router;
pub mod testing;
pub mod types;

pub use crate::config::{ConfigError, EdgeConfig, Resolution};
pub use crate::context::{Context, LogLevel};
pub use crate::error::{Error, FetchError, KvError, PathError};
pub use crate::kv::{KvStore, KvValue};
pub use crate::router::{handler, Handler, RouteParams, Router};
pub use crate::types::{
    Body, ChunkStream, EdgeRequest, EdgeResponse, HeaderMap, HeaderName, HeaderValue, Method,
    ResponseExt, StatusCode, Uri, Url, Version,
};

/// The platform entry macro: `#[edge_core::main]` (SPEC §6.2).
///
/// Expands to the workers-rs fetch glue under `--features cloudflare` and
/// the Fastly sync entry under `--features fastly`. The two features are
/// mutually exclusive.
pub use edge_macros::main;

/// Result alias defaulting to [`Error`].
pub type Result<T> = std::result::Result<T, Error>;
