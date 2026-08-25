# Rate limiting (§9)

Split from `SPEC-PORTABILITY-PRIMITIVES.md` §9 (2026-08-25). Part of the
[portability primitives](README.md) (draft v0.2). Original section number
preserved as a heading so references ("§9") keep resolving.

Cloudflare's native rate-limit bindings and Fastly's edge rate counters plus penalty boxes implement one application-facing contract:

```rust
pub enum RateLimitFailureMode {
    Open,
    Closed,
}

pub enum RateLimitFailure {
    Timeout,
    Platform,
}

pub struct RateLimitOptions {
    pub timeout: Duration,
    pub failure_mode: RateLimitFailureMode,
}

pub struct RateLimitOutcome {
    pub allowed: bool,
    pub estimated_rate: Option<u64>,
    pub failure: Option<RateLimitFailure>,
}

pub struct RateLimiter { /* named binding */ }

impl Context {
    pub fn rate_limiter(&self, name: &str) -> Result<RateLimiter>;
}

impl RateLimiter {
    pub async fn check(
        &self,
        key: &str,
        options: RateLimitOptions,
    ) -> RateLimitOutcome;
}
```

Rules:

- The facade returns an outcome; the application decides whether to deny, observe, sample, or exempt.
- Timeout and platform failure are distinguishable from an exceeded policy.
- Fail-open or fail-closed behavior is explicit.
- Empty keys are rejected rather than mapped to a global bucket.
- Names identify configured policies, not provider objects.
- A batch API SHOULD permit independent checks to run concurrently and return outcomes by caller label.

Shared configuration expresses total requests per period:

```toml
[rate_limits.login]
limit = 600
period_seconds = 60
mitigation_seconds = 120
```

Platform mapping:

- **Cloudflare:** generate a native rate-limit binding and invoke it by key.
- **Fastly:** use an edge rate counter and penalty box. Check the penalty box, update/read the counter, compare it with the normalized policy, and penalize an exceeded key for `mitigation_seconds`.
- The Fastly adapter owns conversion from total requests per period to the provider's rate representation.
- Exact algorithms may differ. The guarantee is policy-level: normal traffic is allowed, sustained excess is denied for the mitigation interval, and failures follow the selected mode.
- Config validation rejects periods or mitigation durations unsupported by either selected target.
