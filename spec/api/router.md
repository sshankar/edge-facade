# Router (§6.6)

Split from `SPEC.md` §6.6 (2026-08-25). Part of the [API surface](README.md).

```rust
pub struct Router;
pub struct RouteParams<'a> { /* extractor */ }

impl Router {
    pub fn new() -> Self;
    pub fn route(&mut self, pattern: &str, handler: Handler) -> Result<()>; // matchit patterns, e.g. "/hello/:name"
    pub async fn handle(&self, req: EdgeRequest, ctx: &mut Context) -> Result<EdgeResponse>; // 404 if unmatched
}
```

- Pattern syntax: matchit (same as workers-rs).
- No axum in v1 ([D7](../decisions/d07.md)).
- Router is a convenience; plain `match req.uri().path()` remains fully supported.

## See also

- [D7](../decisions/d07.md) — own matchit router, no axum
- [D12](../decisions/d12.md) — router misses become 404 at the adapter boundary
