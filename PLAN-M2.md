# M2 Implementation Plan — `edge-cloudflare` adapter

**Status: ✅ COMPLETE (2026-08-22)** — all acceptance criteria met: `edge-cloudflare` adapter + real `#[edge_core::main]` cloudflare glue implemented; conformance T1–T4 **and** hello-world pass under workerd; M1 Viceroy suite still green; 52 host tests green; clippy/fmt/doc clean; both wasm targets check clean.

**Goal (spec §12, M2):** `edge-cloudflare` adapter, such that hello-world and T1–T3 pass under workerd, then live CF.

**Definition of done:**
- `edge-cloudflare` adapter (SPEC §8.1): request/response conversion via the worker `http`-feature helpers with buffered bodies (D2), `Context` from `Env` bindings (var/secret/kv), global `fetch` with `redirect: manual` (D5.2), log → console macros, §6.4 error mapping.
- `#[edge_core::main]` cloudflare branch: real workers-rs fetch glue (`#[wasm_bindgen]` export + `future_to_promise(AssertUnwindSafe(...))`), errors → 500/404 (D12).
- Service crates restructured: cdylib lib (CF) + thin fastly bin; `cloudflare` feature wired.
- trybuild "both features" fixture added.
- Toolchain: `worker-build` 0.8.5 (crates.io) + `workerd` prebuilt; conformance T1–T4 + hello-world green under workerd.

---

## 1. What was built

```
edge/
├── edge-cloudflare/          # NEW: Cloudflare adapter (worker = "=0.8.5", features = ["http"])
│   └── src/{lib,convert,platform,kv,send}.rs
├── edge-macros/              # cloudflare branch: real glue; both-features fixture
├── examples/hello-world/     # lib (cdylib) + fastly bin; workerd-hello-world.capnp
└── tests/conformance/        # CF entry in lib; workerd-conformance.capnp; run-cf.sh
```

## 2. Deviations from SPEC (back-ported to SPEC §13, D16–D20)

1. **D16 — CF fetch error granularity.** JavaScript fetch rejections are `Error` objects with no
   typed cause (unlike Fastly's `SendErrorCause`), so the adapter maps them to
   `FetchError::Connection(js_message)`; the "lossless at the category level" requirement (SPEC
   §6.4) cannot be met more precisely on CF — documented asymmetry.
2. **D17 — empty-body normalization.** `web_sys::Request` throws
   `TypeError: Request with a GET or HEAD method cannot have a body` when the init carries a
   present-but-empty stream (verified under workerd, T4). The adapter maps empty `Bytes` to a
   *null* body (`worker::Body::empty()`), so an empty fetch body produces no body on the wire —
   the parity behavior both platforms already share.
3. **D18 — `Send` bridging for workers-rs futures.** `JsFuture`-based futures capture
   `Rc<RefCell<…>>` and are `!Send`, while the core SPI (`BoxFuture`, `Platform: Send + Sync`)
   requires `Send`. The adapter wraps them in a documented unsafe `Send` marker — sound because
   the wasm runtime is single-threaded and the future is driven to completion on its creating
   thread (the same justification workers-rs uses for its own `unsafe impl Send`).
4. **D19 — service crates require `wasm-bindgen` as a direct dependency.** The `#[wasm_bindgen]`
   macro's generated code references `::wasm_bindgen`, which must be in the service crate's extern
   prelude. Wired as an optional dep under the `cloudflare` feature (same version as `worker`'s,
   unified by semver).
5. **D20 — cdylib lib + bin split.** `worker-build` requires a `cdylib` lib target; wasip1 needs a
   bin entry. Service crates now have a shared lib (handler + `#[edge_core::main]` under
   `--features cloudflare`) and a 6-line fastly bin with its own `#[edge_core::main]`. One handler
   source, two entries; the macro's generated fastly `main` stays private (bin-local).

## 3. Test plan (all green)

| Target | What | Result |
|---|---|---|
| workerd | `tests/conformance/run-cf.sh` — T1 echo, T2 status/UTF-8, T3 router/404, **T4 fetch Host-parity**, hello-world routes | all passed |
| Viceroy | `tests/conformance/run.sh` (M1 regression after restructure) | all passed |
| Host | `cargo test` (52) incl. trybuild both-features fixture (`--features fastly,cloudflare`) | passed |
| wasm | `edge-cloudflare` on `wasm32-unknown-unknown`; `edge-fastly` on `wasm32-wasip1`; services on both targets | clean |
| Lints | clippy 0 warnings (5 lib crates + both service feature sets), fmt, doc | clean |

**T4 under workerd:** the worker's `globalOutbound` is an `ExternalServer` pointing at the local
echo origin; workerd forwards fetch() there **keeping the original Host header**
(`api.example.com`), which is exactly what the T4 assertions check (D5.1) and what CF production
does natively.

**Not yet testable:** live CF deploy (no account/credentials here); CF KV and vars/secrets (T7/T8,
M4 — the `CloudflareKvBackend` code is written but needs a workerd `kvNamespaces` harness).

## 4. How to run

```bash
# Host tests
cargo test

# Cloudflare build + run under workerd (worker-build + workerd on PATH)
tests/conformance/run-cf.sh

# Fastly regression (viceroy on PATH)
tests/conformance/run.sh

# Feature-matrix compile errors
cargo test -p edge-macros --features fastly,cloudflare
```

## 5. Notes for M3/M4/M5

- **M4** adds vars/secrets/KV conformance (T7/T8): workerd config needs `kvNamespaces` (plus the
  `Workerd.KvNamespace`-style service) and text/data bindings for vars/secrets; Viceroy needs
  `[local_server.config_stores|kv_stores|secret_stores]`.
- **M3** owns T5/T6/T11 on both platforms; on CF the "undeclared host" test is a plain fetch
  (fail-open) — the asymmetry documented in §7.5.
- **worker-build** pins a wasm-bindgen version it downloads at build time; if that version moves
  ahead of `worker` 0.8.5's, pin via `WASM_BINDGEN_VERSION`/lockfile (see worker-build docs).

## 6. Explicitly out of scope for M2

Live CF deploy, KV/vars/secrets tests (M4), T5/T6/T10/T11, `edge-cli` (M5), CI matrix (M5),
streaming.
