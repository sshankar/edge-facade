# Configuration (§10)

Split from `SPEC-PORTABILITY-PRIMITIVES.md` §10 (2026-08-25). Part of the
[portability primitives](README.md) (draft v0.2). Original section number
preserved as a heading so references ("§10") keep resolving.

```toml
[runtime]
request_timeout_ms = 20000
max_deferred_tasks = 32
deferred_budget_ms = 30000

[logging]
endpoint = "default_logging"
max_fields_bytes = 98304

[[schedules]]
name = "dictionary-prewarm"
cron = "*/4 * * * *"
handler = "prewarm"
fastly_delivery = "authenticated_http"

[rate_limits.login]
limit = 600
period_seconds = 60
mitigation_seconds = 120
```

Application timeouts may be stricter than config but cannot extend a platform or request deadline. Maintenance-route credentials remain secret bindings and MUST NOT be emitted into generated config.
