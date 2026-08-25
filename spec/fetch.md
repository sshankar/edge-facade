# Fetch & backend resolution (§7)

URL → transport: how `Context::fetch` turns an absolute URL into a platform
subrequest. Split from `SPEC.md` §7 (2026-08-25). Decisions: [D1](decisions/d01.md)
(URL-first), [D4](decisions/d04.md) (fail-closed), [D5](decisions/d05.md)
(parity), [D10](decisions/d10.md) (resolution policy in core).

## 7.1 API contract

`Context::fetch(req)` takes a complete `http::Request` with an **absolute URI**. Relative URIs → `Error::Fetch(FetchError::BadRequest)`.

## 7.2 Shared origin config

Single source of truth, committed alongside the handler:

```toml
# edge.toml (schema v1)
[service]
name = "hello-world"

[origins]
api = { url = "https://api.example.com", backend = "api_backend" }

[stores]
kv = "edge_kv"                    # namespace name on CF, store name on Fastly

[logging]
endpoint = "default_logging"      # fastly log endpoint; ignored on CF

[fastly]
dynamic_backends = false          # MUST be explicit
```

`edge-cli` codegen produces, from this file:
- `fastly.toml`: `[setup.backends]` (keyed by backend name, each with `override_host = "<url host>"` and `use_ssl` per scheme) plus `[setup.kv_store]`/config/secret-store entries and `[logging]` endpoints. An existing `[local_server]` section is preserved (Viceroy testing config).
- `wrangler.toml`: `main = "build/index.js"`, `compatibility_date`, and a `[[kv_namespaces]]` binding per `[stores] kv` — without an `id` (wrangler auto-provisions and writes ids back on deploy). Vars/secrets are configured per-environment, not generated (secrets via `wrangler secret put`). No fetch-permission config is emitted: wrangler has no allowlist key (see [questions](questions.md) #1).

The same map is embedded in the binary at build time (via a `build.rs`-generated module or `include_str!` + a `serde` deserializer) for runtime resolution. The runtime map is the **primary** resolution source on Fastly; it is authoritative and MUST match `fastly.toml`.

## 7.3 Resolution chain (Fastly adapter)

For `req.uri().host()` H, port P, scheme S:

1. **Static match:** if `origins[H]` exists → `Request::send(backend_name)` (validate with `Backend::from_name` for a friendly `UnresolvedBackend` error).
2. **Dynamic fallback:** else if `[fastly] dynamic_backends = true` → `Backend::builder(name, "H:P")` with:
   - `enable_ssl()` if S == https (SNI = H), plain if http
   - `override_host(H)` (parity requirement, §7.4)
   - `connect_timeout`/`first_byte_timeout`/`between_bytes_timeout` from defaults (configurable later)
   - `finish()`, handling `BackendCreationError::Disallowed` → `FetchError::Permission`
   - Cache per-session: `OnceLock<HashMap<String /*host*/, Backend>>` (dynamic backends are per-session entities; names may overlap across sessions, so per-session caching is correct).
3. **Else:** `FetchError::UnresolvedBackend(H)` (fail closed).

## 7.4 Behavioral parity rules (MUST)

1. **Host header / SNI identity:** CF sends the URL host upstream. Fastly connects to the backend address and uses backend host by default. The adapter MUST ensure the origin receives `Host: H` — via `override_host(H)` on dynamic backends and `override_host` in generated `fastly.toml` for static backends. Empirically verified in M3 (Viceroy + workerd echo-origin test).
2. **Redirects:** CF `fetch` follows by default; Fastly `send` does not. The adapter MUST set CF redirect policy to `manual` so redirect handling is identical (none) on both platforms.
3. **Path/query:** preserved verbatim; only transport differs.
4. **Request headers:** pass through except hop-by-hop normalization differences — MUST be documented and tested (e.g. `connection`, `keep-alive`).

## 7.5 Error behavior

- Unresolved host → `FetchError::UnresolvedBackend` (Fastly) vs pass-through behavior (CF). Handlers MUST NOT assume both platforms reach the origin for arbitrary undeclared hosts; the conformance suite tests the declared-origin path and the fail-closed path.

## See also

- [config](config.md) — edge.toml schema, edge-cli codegen (§9/§10)
- [adapters/cloudflare](adapters/cloudflare.md) · [adapters/fastly](adapters/fastly.md) — transport per platform
- [conformance](conformance.md) — T4/T5/T6/T11 scenarios
