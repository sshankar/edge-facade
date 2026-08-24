# M6 Implementation Plan — Streaming response bodies

**Status: ✅ COMPLETE (2026-08-24)** — `Body` is now an enum
(`Buffered`/`Streaming`) with a poll-based `ChunkStream` trait,
`Context::fetch_streaming` returns headers + a streaming body on all three
platforms, and adapters stream `Streaming` responses to the client. T12
(stream + relay) passes identically on host (mock) and under Viceroy; the
workerd suite is compile-checked and wired into `run-cf.sh` + CI.

**Goal (spec §12, M6):** streaming response bodies — large-file relay,
incremental reads — without breaking the v1 buffered contract or the D3
poll-loop executor.

**Definition of done:**
- `Body` enum with `Buffered(Bytes)` + `Streaming(Box<dyn ChunkStream>)`;
  buffered bodies double as one-shot streams (re-boxable, relaying works).
- `Context::fetch_streaming` — headers-first response with streaming body;
  `Context::fetch` keeps returning buffered bodies (zero breakage).
- Fastly: `send` → body handle kept live → `stream_to_client()` for the
  relay (genuinely chunked — verified under Viceroy).
- CF: `worker::Body` ReadableStream kept live → `Response::from_stream`.
- Mock: `fetch_streaming` presents a stream even for buffered origins
  (parity); `fetch` drains streams (buffered contract).
- Conformance T12 on all three targets + native unit tests + core tests.

---

## 1. Why no select-scheduler (D3 remains intact)

SPEC D2 originally deferred streaming because "streaming is incompatible
with the D3 executor contract". That applies to *concurrent* streams
(select-based scheduling, M7). **Sequential** streaming works under the
poll-loop executor:

- On Fastly, chunk reads are **blocking host calls** (`fastly::Body` reads).
  The adapter's `ChunkStream::poll_next_chunk` reads synchronously and
  returns `Ready` on the first poll — exactly like every other `Context`
  method (SPEC §8.3). The drive loop never sees `Pending`.
- On CF, reads are genuinely async (ReadableStream); the JS event loop
  drives them natively.
- The handler-to-client relay loop is a sequential read→write loop, also
  synchronous on Fastly (`stream_to_client` + `Write`).

This is recorded as SPEC decision D21.

## 2. What was built

```
edge-core/src/types.rs           # Body enum, ChunkStream trait, next_chunk/
                                 #   collect/stream/once/from_chunks, From
                                 #   impls, ResponseExt::text error on streams
edge-core/src/context.rs         # Platform::fetch_streaming (default = fetch)
                                 #   + Context::fetch_streaming
edge-core/src/kv.rs              # KvBackend::put takes Bytes; KvStore::put
                                 #   drains streaming bodies; KvValue(Bytes)
edge-core/src/testing/mod.rs     # mock fetch_streaming (one-shot for buffered
                                 #   origins), fetch drains streams,
                                 #   request recording without body clone
edge-fastly/src/convert.rs       # FastlyChunkStream, from_fastly_streaming,
                                 #   response_to_client streaming branch
edge-fastly/src/platform.rs      # fetch_streaming (same host `send`)
edge-cloudflare/src/convert.rs   # WorkerChunkStream, response_to_edge_streaming,
                                 #   EdgeBodyStream + from_stream branch
edge-cloudflare/src/platform.rs  # fetch_streaming (headers via JsFuture)
tests/conformance/               # T12 handler + route, origin.py /t12-origin,
                                 #   native tests, run.sh/run-cf.sh blocks
SPEC.md                          # §2/§3/§6.1/§11/§12/D2/D21 updated
PLAN-M6.md                       # this file
```

## 3. Design decisions (see SPEC D21)

1. **Poll-based `ChunkStream` trait** (`poll_next_chunk`), not an async
   iterator: object-safe, works under both the D3 poll-loop (Fastly) and the
   JS event loop (CF), and bridges trivially to `http_body`-style polling.
2. **`fetch_streaming` is additive**; `fetch` semantics unchanged (buffered).
   Adapters always present a streaming body from `fetch_streaming` — even a
   tiny origin response — so handlers can rely on it. The mock wraps
   buffered origin bodies as one-shot streams for parity.
3. **Request bodies stay buffered** (D2). `send_async_streaming` upload
   streaming is out of scope (needs a two-phase API; revisit with M7).
4. **Chunk boundaries are not portable.** T12 asserts the invariant
   consumed-first-chunk + relayed-body == full payload, which holds at any
   granularity. run.sh additionally asserts `transfer-encoding: chunked`
   (no Content-Length) on the Fastly relay — the streaming proof.
5. **KV values are bytes.** `KvBackend::put` takes `Bytes`; `KvStore::put`
   drains streaming bodies first. `KvValue` wraps `Bytes` (was `Body`).
6. **Request recording in the mock** deep-copies parts (bodies are no longer
   `Clone` — streams can't clone); buffered bodies are preserved.

## 4. Verification

| Check | Result |
|---|---|
| `cargo test --workspace` | 22 suites ok, 0 failures (8 new core streaming tests, 2 new T12 native tests) |
| `cargo clippy --workspace --all-targets -D warnings` | 0 warnings |
| `cargo fmt --check` | clean |
| `cargo doc --workspace --no-deps` | clean |
| `cargo build` wasm32-wasip1 (fastly) + wasm32-unknown-unknown (cloudflare) | clean |
| `run.sh` (Viceroy) | T1–T8, r1, T11, **T12** all pass; T12 relay observed `transfer-encoding: chunked`, `x-t12-first-chunk: 8192`, relay 14349 bytes, invariant holds (22541 total) |
| `run-cf.sh` (workerd) | not run locally (no node/workerd on this host); T12 block mirrors the proven invariant; CI job covers it |

## 5. Risks & notes

- **workerd T12 unexercised locally.** The CF path is compile-verified
  (wasm32-unknown-unknown) and mirrors the Fastly flow; `run-cf.sh` asserts
  only the platform-independent invariant (no chunked-encoding assertion —
  workerd's framing for `from_stream` is not verified here). First CI run
  will confirm.
- **Source-level break for v1 users:** `resp.body()` no longer derefs to
  `&[u8]`; fetch sites use `.as_bytes()` (buffered) or `collect()`/`next_chunk()`.
  Bounded and grep-able (SPEC D21 "Consequences").
- Streaming request bodies, concurrent streams, and `send_async_streaming`
  uploads remain M7 territory (`SPEC-PORTABILITY-PRIMITIVES.md` §4.1).
