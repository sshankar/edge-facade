# Overview

Purpose, non-goals, design principles, and workspace layout for the Edge
SDK. Split from `SPEC.md` §1/§2/§4/§5 (2026-08-25). See the
[wiki index](README.md) for the full catalog.

## 1. Purpose

Enable a single Rust handler, written once, to be compiled and deployed as:

| Platform | Wasm target | Runtime interface | Tooling |
|---|---|---|---|
| Cloudflare Workers | `wasm32-unknown-unknown` | JS host via wasm-bindgen (`worker` crate) | `worker-build`, `wrangler` |
| Fastly Compute | `wasm32-wasip1` / `wasm32-wasip2` | Pure WASI host ABI via `fastly-sys` | `fastly compute` |

The SDK provides:

1. A platform-agnostic core (types, handler contract, context, router, error model).
2. Per-platform adapters that convert between the core model and the native SDK.
3. A `#[edge::main]` entry macro that expands to the correct platform glue.
4. A shared configuration model and codegen so a single config produces both `wrangler.toml` and `fastly.toml`.

## 2. Non-goals (v1)

WebSockets, Durable Objects, Queues, R2, D1, Fanout, image optimizer, device detection, scheduled/cron events, streaming *request* bodies, platform-specific geo and cache APIs, HTTP/2 push, service bindings. Anything listed here is excluded unless a later version explicitly adopts it. Streaming *response* bodies were adopted in M6 (SPEC D21 — [decisions/d21](decisions/d21.md)); `Context::fetch` still buffers (v1 semantics) and `Context::fetch_streaming` exposes the streaming path.

**Supersession (draft v0.2):** `SPEC-PORTABILITY-PRIMITIVES.md` (`portability/README` — planned) adopts *scheduled/cron events* and *client metadata (geo, network, TLS)* as portable primitives, superseding the exclusions above to the extent described there. Streaming request bodies and platform-specific cache APIs remain excluded.

## 4. Design principles

1. **URL-first, never backend-first.** Handlers express intent with URLs; platform transport details are adapter concerns ([D1](decisions/d01.md)).
2. **Buffered bodies in v1.** `Bytes` everywhere; streaming deferred to v2 ([D2](decisions/d02.md), superseded for responses by [D21](decisions/d21.md)).
3. **Platform deps only in adapters.** `edge-core` depends only on `std` + `http` + `bytes` + `serde` + `serde_json` + `matchit` (+ `async-trait` if used).
4. **One handler, feature-selected platform.** `--features cloudflare` xor `--features fastly`; mutually exclusive.
5. **Behavioral parity, not API parity.** Where platforms differ semantically (Host header, redirects, KV limits), the SDK normalizes to the *stricter/most common* behavior and documents it ([D5](decisions/d05.md)).
6. **Fail closed on Fastly.** Undeclared fetch hosts are an error unless dynamic backends are explicitly enabled in config ([D4](decisions/d04.md)).

## 5. Workspace layout

```
edge/
├── Cargo.toml            # workspace
├── spec/                 # this wiki (spec + decisions + milestone roadmap)
├── edge-core/            # platform-agnostic: types, Context, Router, errors, config schema
├── edge-macros/          # #[edge::main] attribute macro
├── edge-cloudflare/      # adapter: depends on worker crate (wasm32-unknown-unknown only)
├── edge-fastly/          # adapter: depends on fastly crate (wasi targets only)
├── edge-cli/             # codegen: edge.toml → wrangler.toml + fastly.toml
├── examples/hello-world/ # one handler, both platforms
└── tests/conformance/    # shared conformance suite (see conformance)
```

**Dependency rules (MUST):**
- `edge-core` compiles on host, `wasm32-unknown-unknown`, and `wasm32-wasip1`/`wasip2`.
- `edge-cloudflare` compiles only for `wasm32-unknown-unknown`; `edge-fastly` only for `target_env = "p1" | "p2"`. Enforced via `[target.'cfg(...)'.dependencies]`.
- `edge-fastly` pins `fastly = "=0.13.0"`, `fastly-macros = "=0.13.0"`, `fastly-sys = "=0.13.0"` (lockstep publishes).
- `edge-cloudflare` pins `worker = { version = "0.8.5", features = ["http"] }`.
- MSRV: 1.88 (Fastly's requirement).

## See also

- [capability-matrix](capability-matrix.md) — per-platform ground truth (§3)
- [questions](questions.md) — open questions & risks (§14)
