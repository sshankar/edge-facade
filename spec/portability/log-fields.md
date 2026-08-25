# Structured logging fields (§6)

Split from `SPEC-PORTABILITY-PRIMITIVES.md` §6 (2026-08-25). Part of the
[portability primitives](README.md) (draft v0.2). Original section number
preserved as a heading so references ("§6") keep resolving.

Line logging and request-level fields are separate facilities:

```rust
impl Context {
    pub fn set_log_field(
        &self,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<()>;

    pub fn remove_log_field(&self, key: &str);
}
```

Contract:

- Fields are invocation-scoped strings.
- Keys are normalized to lowercase ASCII and validated against `[a-z0-9][a-z0-9._-]*`.
- Empty values are omitted; setting an existing key replaces it.
- Per-value and aggregate byte budgets are documented and enforced.
- Budget truncation is deterministic and emits a diagnostic.
- Fields are emitted for successful, synthetic, timeout, and catch-all responses.
- Origin responses cannot inject the platform logging control field.
- Logging fields are not client-visible.
- Applications own allowlists, sensitive-data classification, and schema generation.

Platform mapping:

- **Cloudflare:** serialize fields into the platform control response header (`x-edge-log-fields`, a JSON object with sorted keys) with correct escaping, after stripping an origin-provided value (diagnostic emitted). The header is the platform's boundary record — Cloudflare has no out-of-band log endpoint; the workerd harness reads it as the finalized map.
- **Fastly:** emit one structured record (`{"fields": {...}}`) to the configured log endpoint (stderr fallback) during finalization, for every request outcome; the control header never reaches the client.
- **Tests:** the mock exposes the finalized map to the harness (`Records::finalized_log_fields`) and the serialized control value; drivers verify no origin control data reaches the client.
- **Shared policy (decision D22, M11):** keys normalized to lowercase ASCII and validated against `[a-z0-9][a-z0-9._-]*`; empty values omitted; per-value budget 1024 bytes (char-boundary truncation); aggregate budget 4096 bytes (oldest dropped, newest retained — deterministic); truncation emits a diagnostic. Serialized form is a JSON object with lexicographically sorted keys.
