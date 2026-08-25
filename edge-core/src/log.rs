//! Logging macros and structured logging fields (SPEC-PORTABILITY-PRIMITIVES §6, M11).
//!
//! # Logging macros
//!
//! Usage: pass the [`crate::Context`] as the first argument.
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
//!
//! # Structured log fields
//!
//! `Context::set_log_field` / `Context::remove_log_field` manage
//! invocation-scoped string fields that adapters emit at the response
//! boundary: Cloudflare serializes them into the control response header
//! (after stripping an origin-provided value); Fastly writes one structured
//! JSON record to the configured log endpoint during finalization; the mock
//! exposes the finalized map to the harness.
//!
//! The policy is shared by every platform so behavior is identical on all
//! three targets (P9–P11):
//!
//! - Keys are normalized to lowercase ASCII and validated against
//!   `[a-z0-9][a-z0-9._-]*`.
//! - Empty values are omitted.
//! - Setting an existing key replaces it.
//! - Per-value budget: [`VALUE_BUDGET_BYTES`] bytes (truncated at a UTF-8
//!   char boundary). Aggregate budget: [`TOTAL_BUDGET_BYTES`] bytes of
//!   `key.len() + value.len()`; on overflow the oldest fields are dropped
//!   (newest retained), deterministically. Both truncations emit a
//!   diagnostic.
//! - Serialized form is a JSON object with lexicographically sorted keys
//!   (deterministic across platforms).

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

use crate::context::{Context, LogLevel};
use crate::types::EdgeResponse;

/// The control response header carrying log fields at the adapter boundary.
///
/// Origin/handler-supplied values of this header are stripped before the
/// response reaches the client (P10); on Cloudflare the adapter's own
/// serialized fields ride in it (the conformance harness reads it there as
/// the finalization record); on Fastly the record goes to the log endpoint
/// and the header never reaches the client.
pub const CONTROL_HEADER: &str = "x-edge-log-fields";

/// Per-value byte budget (SPEC-PORTABILITY-PRIMITIVES §6).
pub const VALUE_BUDGET_BYTES: usize = 1024;

/// Aggregate byte budget — the sum of `key.len() + value.len()` over all
/// fields (SPEC-PORTABILITY-PRIMITIVES §6).
pub const TOTAL_BUDGET_BYTES: usize = 4096;

/// An ordered map of invocation-scoped log fields with the facade's budget
/// policy applied. Shared by all platforms (host mock, Cloudflare, Fastly)
/// so normalization, validation, budgets, and diagnostics are identical.
#[derive(Debug, Default)]
pub struct LogFieldMap {
    fields: Vec<(String, String)>,
    bytes: usize,
    diagnostics: Vec<String>,
}

impl LogFieldMap {
    /// A new empty map.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set a field, replacing any existing value with the same key.
    ///
    /// Empty values are omitted (SPEC §6). Invalid keys fail with
    /// [`crate::Error::LogField`]. Budget truncation is deterministic and
    /// recorded as a diagnostic (drained via
    /// [`LogFieldMap::drain_diagnostics`]).
    pub fn set(&mut self, key: String, value: String) -> crate::Result<()> {
        let key = normalize_key(&key)?;
        if value.is_empty() {
            // Empty values are omitted (SPEC-PORTABILITY-PRIMITIVES §6).
            return Ok(());
        }
        let value_original_len = value.len();
        let value = truncate_value(&value);
        if value.len() < value_original_len {
            self.diagnostics.push(format!(
                "log field `{key}` exceeded the {VALUE_BUDGET_BYTES}-byte per-value budget; truncated"
            ));
        }
        if let Some(i) = self.fields.iter().position(|(k, _)| k == &key) {
            let (_, old) = self.fields.remove(i);
            self.bytes = self.bytes.saturating_sub(old.len());
        }
        self.bytes += key.len() + value.len();
        self.fields.push((key, value));
        if self.bytes > TOTAL_BUDGET_BYTES {
            self.evict_oldest();
        }
        Ok(())
    }

    /// Remove a field by key (normalized; a no-op if absent).
    pub fn remove(&mut self, key: &str) {
        let Ok(key) = normalize_key(key) else {
            return;
        };
        if let Some(i) = self.fields.iter().position(|(k, _)| k == &key) {
            let (_, old) = self.fields.remove(i);
            self.bytes = self.bytes.saturating_sub(key.len() + old.len());
        }
    }

    /// Whether no fields are set.
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    /// An ordered snapshot of the current fields (insertion order).
    pub fn snapshot(&self) -> Vec<(String, String)> {
        self.fields.clone()
    }

    /// Take any pending budget diagnostics (e.g. to emit through the log
    /// sink).
    pub fn drain_diagnostics(&mut self) -> Vec<String> {
        std::mem::take(&mut self.diagnostics)
    }

    /// Serialize the fields for the adapter boundary: a JSON object with
    /// lexicographically sorted keys, or `None` when no fields are set.
    pub fn serialize(&self) -> Option<String> {
        if self.fields.is_empty() {
            return None;
        }
        // serde_json::Map is a BTreeMap without the `preserve_order`
        // feature, so keys serialize sorted — deterministic across calls.
        let mut map = serde_json::Map::new();
        for (k, v) in &self.fields {
            map.insert(k.clone(), serde_json::Value::String(v.clone()));
        }
        Some(serde_json::Value::Object(map).to_string())
    }

    /// Drop oldest fields (insertion order) until the aggregate budget fits.
    /// The most recently set field is always retained.
    fn evict_oldest(&mut self) {
        let mut dropped = 0usize;
        while self.bytes > TOTAL_BUDGET_BYTES && self.fields.len() > 1 {
            let (k, v) = self.fields.remove(0);
            self.bytes = self.bytes.saturating_sub(k.len() + v.len());
            dropped += 1;
        }
        if dropped > 0 {
            self.diagnostics.push(format!(
                "log field aggregate budget ({TOTAL_BUDGET_BYTES} bytes) exceeded; \
                 dropped {dropped} oldest field(s) (newest retained)"
            ));
        }
    }
}

/// Normalize a key to lowercase ASCII and validate it against
/// `[a-z0-9][a-z0-9._-]*`.
fn normalize_key(key: &str) -> std::result::Result<String, crate::Error> {
    let key = key.to_ascii_lowercase();
    let mut chars = key.chars();
    let first = chars
        .next()
        .ok_or_else(|| crate::Error::LogField("key must not be empty".to_string()))?;
    if !(first.is_ascii_lowercase() || first.is_ascii_digit()) {
        return Err(crate::Error::LogField(format!(
            "invalid log field key `{key}` (must match [a-z0-9][a-z0-9._-]*)"
        )));
    }
    for c in chars {
        if !(c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-')) {
            return Err(crate::Error::LogField(format!(
                "invalid log field key `{key}` (must match [a-z0-9][a-z0-9._-]*)"
            )));
        }
    }
    Ok(key)
}

/// Truncate a value to [`VALUE_BUDGET_BYTES`] bytes at a UTF-8 char
/// boundary.
fn truncate_value(value: &str) -> String {
    if value.len() <= VALUE_BUDGET_BYTES {
        return value.to_string();
    }
    let mut out = String::new();
    for ch in value.chars() {
        if out.len() + ch.len_utf8() > VALUE_BUDGET_BYTES {
            break;
        }
        out.push(ch);
    }
    out
}

/// Strip an origin/handler-supplied control header from a response before it
/// reaches the client, emitting a diagnostic (SPEC-PORTABILITY-PRIMITIVES
/// §6, P10). Returns whether a header was stripped.
///
/// Used by the Fastly adapter (and by native tests simulating the adapter
/// boundary). On Cloudflare the equivalent operates on `web_sys::Response`
/// headers in the adapter.
pub fn strip_control_header(resp: &mut EdgeResponse, ctx: &Context) -> bool {
    if resp.headers_mut().remove(CONTROL_HEADER).is_some() {
        ctx.log(
            LogLevel::Warn,
            "stripped client-visible logging control header from the response \
             (origin-supplied value ignored)",
        );
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map() -> LogFieldMap {
        LogFieldMap::new()
    }

    #[test]
    fn keys_are_normalized_to_lowercase() {
        let mut m = map();
        m.set("Request-ID".into(), "abc".into()).unwrap();
        assert_eq!(m.snapshot(), vec![("request-id".into(), "abc".into())]);
    }

    #[test]
    fn invalid_keys_are_rejected() {
        let mut m = map();
        assert!(m.set("has space".into(), "v".into()).is_err());
        assert!(m.set("1bad".into(), "v".into()).is_ok());
        assert!(m.set("".into(), "v".into()).is_err());
        assert!(m.set("ok.key_1-2".into(), "v".into()).is_ok());
    }

    #[test]
    fn empty_values_are_omitted() {
        let mut m = map();
        m.set("k".into(), "v".into()).unwrap();
        m.set("k".into(), "".into()).unwrap();
        assert_eq!(m.snapshot(), vec![("k".into(), "v".into())]);
    }

    #[test]
    fn setting_existing_key_replaces() {
        let mut m = map();
        m.set("k".into(), "a".into()).unwrap();
        m.set("k".into(), "b".into()).unwrap();
        assert_eq!(m.snapshot(), vec![("k".into(), "b".into())]);
    }

    #[test]
    fn remove_deletes_by_normalized_key() {
        let mut m = map();
        m.set("A".into(), "1".into()).unwrap();
        m.remove("a");
        assert!(m.is_empty());
    }

    #[test]
    fn per_value_budget_truncates_at_char_boundary() {
        let mut m = map();
        let long = "é".repeat(700); // 2 bytes each = 1400 bytes > 1024
        m.set("k".into(), long.clone()).unwrap();
        let got = &m.snapshot()[0].1;
        assert!(got.len() <= VALUE_BUDGET_BYTES);
        assert!(got.is_char_boundary(got.len()));
        assert!(!m.drain_diagnostics().is_empty());
    }

    #[test]
    fn aggregate_budget_keeps_newest_deterministically() {
        let mut m = map();
        for i in 0..20 {
            m.set(format!("f{i:02}"), "x".repeat(300)).unwrap();
        }
        let snapshot = m.snapshot();
        // 20 * 303 = 6060 > 4096; retained = the 13 newest (13*303 = 3939).
        assert_eq!(snapshot.len(), 13);
        assert_eq!(snapshot.first().unwrap().0, "f07");
        assert_eq!(snapshot.last().unwrap().0, "f19");
        assert!(snapshot.iter().all(|(_, v)| v.len() == 300));
        assert!(m
            .drain_diagnostics()
            .iter()
            .any(|d| d.contains("aggregate budget")));
    }

    #[test]
    fn serialize_is_sorted_json_object() {
        let mut m = map();
        m.set("zeta".into(), "1".into()).unwrap();
        m.set("alpha".into(), "héllo 世界".into()).unwrap();
        let s = m.serialize().unwrap();
        assert_eq!(s, r#"{"alpha":"héllo 世界","zeta":"1"}"#);
    }

    #[test]
    fn serialize_none_when_empty() {
        assert!(map().serialize().is_none());
    }
}
