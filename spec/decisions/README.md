# Decisions (§13)

One page per design decision (`dNN.md`), each with status, decision,
alternatives, rationale, consequences, and the trigger that reopens it.
Original decision numbers are preserved in the page headings so code
references ("SPEC D21") keep resolving. See the [wiki index](../README.md)
for the full catalog. Split from `SPEC.md` §13 (2026-08-25).

| # | Decision | Area |
|---|---|---|
| [D01](d01.md) | URL-first fetch API (never backend-first) | Fetch |
| [D02](d02.md) | Fully buffered `Bytes` bodies in v1 (no streaming) | HTTP types |
| [D03](d03.md) | Immediate-resolution async on Fastly (no executor in v1) | Execution |
| [D04](d04.md) | Fail-closed backend resolution on Fastly | Fetch |
| [D05](d05.md) | Behavioral parity by normalization (Host identity, redirects, KV not-found) | Parity |
| [D06](d06.md) | Single source of truth config (`edge.toml`) with codegen | Config |
| [D07](d07.md) | Own matchit router, no axum, non-`Send` futures in v1 | Routing |
| [D08](d08.md) | API-shape refinements from M0 implementation | API shape |
| [D09](d09.md) | `edge.toml` embedding via the entry macro (no build.rs) | Config |
| [D10](d10.md) | Resolution policy in `edge-core` (`config::Resolution`) | Fetch |
| [D11](d11.md) | `[stores]` config/secrets bindings in the schema | Config |
| [D12](d12.md) | Adapter-level error handling (404 for router misses; infallible serve) | Adapters |
| [D13](d13.md) | Service crates declare both features; CF branch emitted but inert | Build |
| [D14](d14.md) | `edge-fastly` has no host-run tests (link constraint) | Testing |
| [D15](d15.md) | Viceroy config: hand-maintained `[local_server]` + `[setup]` until edge-cli | Testing |
| [D16](d16.md) | CF fetch errors map to `Connection` (no typed causes in JS) | Fetch |
| [D17](d17.md) | Empty-body normalization (CF GET/HEAD) | HTTP types |
| [D18](d18.md) | `Send` bridging for workers-rs futures | Adapters |
| [D19](d19.md) | Service crates depend on `wasm-bindgen` directly | Build |
| [D20](d20.md) | Service crates: cdylib lib + thin fastly bin | Build |
| [D21](d21.md) | Streaming response bodies (M6), no select-scheduler needed | HTTP types |
| [D22](d22.md) | Structured log fields: control-header transport, shared budget policy (M11) | Logging |
| [D23](d23.md) | Client metadata: source mapping per platform (M10) | Metadata |
