# M4 Implementation Plan — Config vars/secrets + KV

**Status: ✅ COMPLETE (2026-08-24)** — all acceptance criteria met: T7 (vars/secrets)
and T8 (KV put/get/delete round trip) pass identically on host (mock), under
Viceroy (`run.sh`), and under workerd (`run-cf.sh`). The Cloudflare adapter is
now config-aware (the `default` KV handle resolves through the embedded
`edge.toml`, matching the Fastly adapter), the workerd KV harness was built
from scratch (kvNamespace binding → a local origin implementing workerd's
KV-over-HTTP protocol), and hello-world's greeting is now config-driven on both
platforms. Clippy/fmt/doc clean; both wasm targets build.

**Goal (spec §12, M4):** Config vars/secrets + KV — T7, T8 on both platforms.

**Definition of done:**
- T7: `Context::var`/`secret` return configured values; unconfigured keys are
  `None` — asserted on all three targets.
- T8: `Context::kv` put/get/delete round trip (incl. binary values and
  missing-key → `None`) — asserted on all three targets.
- CF adapter resolves the `default` KV handle to the binding named in
  `edge.toml [stores] kv` (SPEC §8.1, previously hard-wired to binding
  `"default"`, which never matches generated configs).
- hello-world: greeting (`ctx.var("GREETING")`) is config-driven on both
  simulators (previously always fell back to the hard-coded default).

---

## 1. What was built

```
edge/
├── edge-core/src/context.rs            # DEFAULT_KV_STORE made public (adapter SPI)
├── edge-fastly/src/platform.rs         # uses the shared constant (no behavior change)
├── edge-cloudflare/
│   ├── src/lib.rs                      # serve_fetch(req, env, config, handler)
│   └── src/platform.rs                 # CloudflarePlatform { env, config }; kv("default")
│                                       #   → binding from edge.toml; named → direct
├── edge-macros/src/lib.rs              # cloudflare branch embeds + parses edge.toml (D9),
│                                       #   rejects the fetch promise on bad config
├── examples/hello-world/               # [stores] config binding; GREETING = "Hi" in
│   │                                   #   fastly.toml + workerd capnp
│   └── {edge.toml, fastly.toml, workerd-hello-world.capnp}
└── tests/conformance/
    ├── src/lib.rs                      # t7_config (/t7), t8_kv (/t8); redirect relabeled
    │                                   #   r1_redirect (/r1) — not part of the T-series
    ├── tests/native.rs                 # +2 tests (12 total)
    ├── edge.toml                       # [stores] config/secrets bindings
    ├── fastly.toml                     # [local_server.config_stores|secret_stores|kv_stores]
    ├── origin.py                       # workerd KV-over-HTTP protocol (urlencoded=true);
    │                                   #   HTTP/1.1 + Connection: close
    ├── workerd-conformance.capnp       # text bindings + kvNamespace service
    ├── run.sh / run-cf.sh              # T7/T8/r1 assertions; "Hi, world!"
```

## 2. Harness findings (recorded for M5)

1. **workerd KV namespaces are an HTTP protocol, not a built-in service.** The
   `kvNamespace` binding translates KV operations into HTTP requests against
   the bound service: `GET|PUT|DELETE /<urlencoded-key>?urlencoded=true`
   (verified against `src/workerd/api/kv.c++`). GET 404/410 → missing key;
   2xx → success. The conformance origin implements this in `origin.py`.
2. **workerd's kvNamespace client requires HTTP/1.1 responses.** With
   HTTP/1.0 close-delimited responses the client hangs after the request;
   `Connection: close` + `Content-Length` on HTTP/1.1 responses fixed it.
3. **Secrets must be `text` bindings on CF.** workers-rs `Env::secret` is a
   `StringBinding` (`TYPE_NAME = "String"`); a `data` (ArrayBuffer) binding
   fails the constructor check and the adapter's `.ok()` swallows it. Matches
   real wrangler (secrets are text bindings). `data` bindings are fine for
   other consumers but not `env.secret()`.
4. **rustc dedupes diagnostics:** the CF branch's `include_str!` for
   `edge.toml` emits the same error at the same span as the fastly branch in
   the both-features trybuild fixture, so `both_features.stderr` needed no
   update.

## 3. Test matrix

| Scenario | Native (mock) | Viceroy (run.sh) | workerd (run-cf.sh) |
|---|---|---|---|
| T7 vars/secrets | ✅ (12 tests) | ✅ GREETING/API_KEY/None | ✅ text bindings |
| T8 KV round trip | ✅ | ✅ (inline stores) | ✅ (kvNamespace origin) |
| r1 redirect | ✅ /r1 | ✅ 302 passthrough | ✅ |
| T1–T6, T11 | ✅ | ✅ | ✅ |
| hello-world greeting | — | ✅ "Hi, world!" | ✅ "Hi, world!" |

## 4. Risks & notes

- **KV limits (SPEC §14 #4):** Fastly's KV value size limit is not enforced
  locally (pass-through, documented); CF `put_bytes` uses the ArrayBuffer path
  (D5.3 note in `cloudflare/src/kv.rs`). A size-limit decision is deferred to
  the docs milestone (M5).
- **CF parse-on-fetch:** the macro parses the embedded `edge.toml` per
  invocation (same pattern as the fastly branch, D9). Negligible for v1; a
  `OnceLock` cache can follow if profiling ever shows it.
- **Multi-store on CF:** `kv_named("other")` maps straight to binding
  `"other"` (binding name == store name). Fastly's v1 limitation (only
  `default`) is unchanged (SPEC §6.5).
- **Live deploys** still pending accounts; workerd/Viceroy now cover T7/T8
  fully.
