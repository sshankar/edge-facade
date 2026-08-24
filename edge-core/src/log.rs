//! Logging macros.
//!
//! Usage: pass the [`Context`](crate::Context) as the first argument.
//!
//! ```
//! use edge_core::{Context, LogLevel};
//! use edge_core::log::{error, info, warn};
//! # fn f(ctx: &Context) {
//! info!(ctx, "handled {} requests", 42);
//! warn!(ctx, "rate limit approaching");
//! error!(ctx, "backend failed: {}", "timeout");
//! # }
//! ```
//!
//! On Cloudflare these map to the worker console; on Fastly to the configured
//! logging endpoint; in tests to the mock sink.

/// Log at [`LogLevel::Info`](crate::LogLevel::Info).
#[macro_export]
macro_rules! log_info {
    ($ctx:expr, $($arg:tt)*) => {
        $ctx.log($crate::LogLevel::Info, &format!($($arg)*))
    };
}

/// Log at [`LogLevel::Warn`](crate::LogLevel::Warn).
#[macro_export]
macro_rules! log_warn {
    ($ctx:expr, $($arg:tt)*) => {
        $ctx.log($crate::LogLevel::Warn, &format!($($arg)*))
    };
}

/// Log at [`LogLevel::Error`](crate::LogLevel::Error).
#[macro_export]
macro_rules! log_error {
    ($ctx:expr, $($arg:tt)*) => {
        $ctx.log($crate::LogLevel::Error, &format!($($arg)*))
    };
}

pub use crate::{log_error as error, log_info as info, log_warn as warn};
