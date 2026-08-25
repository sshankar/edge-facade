# Edge SDK

One Rust handler, deployed to **Cloudflare Workers** and **Fastly Compute**.

```
edge/
├── edge-core/        # platform-agnostic: HTTP types, Context, client metadata (M10), log fields (M11), Router, errors, edge.toml config
├── edge-macros/      # #[edge_core::main] entry macro (feature-selected per platform)
├── edge-cloudflare/  # Workers adapter (worker 0.8.5, wasm32-unknown-unknown)
├── edge-fastly/      # Compute adapter (fastly 0.13, wasm32-wasip1)
├── edge-cli/         # codegen + validation: edge.toml → wrangler.toml + fastly.toml
├── examples/hello-world/
└── tests/conformance/  # T1–T8, r1, T11, T12 (streaming, M6) + P7–P11 (M10/M11) — identical on host (mock), Viceroy, workerd
```

## Quick start

```bash
# hello-world: one handler, both platforms
cargo build --target wasm32-wasip1 --features fastly -p hello-world
cargo build --target wasm32-unknown-unknown --features cloudflare -p hello-world

# config codegen (SPEC §9)
edge-cli generate --edge-toml examples/hello-world/edge.toml --out-dir .
edge-cli check    --edge-toml examples/hello-world/edge.toml --fastly-toml fastly.toml
```

## Conformance

The shared suite (`tests/conformance/`) must behave identically on the host
(native mock), under Viceroy, and under workerd (T1–T8, r1, T11, and T12
streaming — first-chunk + relayed body == origin payload):

```bash
cargo test                                  # host (mock) + all unit tests
tests/conformance/run.sh                    # Viceroy (needs viceroy on PATH)
tests/conformance/run-cf.sh                 # workerd (needs worker-build + workerd on PATH)
```

CI runs all three plus the wasm builds (`*.github/workflows/ci.yml`).

## Docs

- `SPEC.md` — the specification (capability matrix, API, adapter contracts,
  conformance suite, milestones, decision log; streaming response bodies are
  decision D21 / milestone M6).
- `SPEC-PORTABILITY-PRIMITIVES.md` — draft v0.2 extension: deferred work,
  deadlines, client metadata, structured log fields, rate limiting,
  scheduled events (milestones M7+).
- `PLAN-M*.md` — per-milestone implementation plans (`PLAN-M10-M11.md` covers client metadata + structured log fields).
