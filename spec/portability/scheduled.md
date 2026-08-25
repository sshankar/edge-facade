# Scheduled events (§8)

Split from `SPEC-PORTABILITY-PRIMITIVES.md` §8 (2026-08-25). Part of the
[portability primitives](README.md) (draft v0.2). Original section number
preserved as a heading so references ("§8") keep resolving.

Scheduled events support optional maintenance such as KV dictionary prewarming, cache refresh, or metrics aggregation. They are not required for ordinary KV reads.

```rust
pub struct ScheduledEvent {
    pub scheduled_at: SystemTime,
    pub cron: Option<String>,
}

pub type ScheduledHandler =
    fn(ScheduledEvent, Context) -> Pin<Box<dyn Future<Output = Result<()>> + Send>>;
```

Applications SHOULD put maintenance logic in a normal async function callable from both a scheduled entry and an authenticated HTTP maintenance route.

Delivery:

- **Cloudflare:** `#[edge::scheduled]` maps to the native scheduled event.
- **Fastly:** no native scheduled ABI is assumed. Deployment invokes the same function through an authenticated internal HTTP endpoint or explicitly configured scheduler.
- **edge-cli:** may generate Cloudflare cron configuration and Fastly maintenance-route metadata, but validation fails when a schedule lacks a Fastly delivery mechanism.

Delivery is at least once, so handlers MUST be idempotent. Overlapping invocations for one service and schedule are disabled unless explicitly configured.
