# edge-cli

Codegen and validation for the Edge SDK's single-source-of-truth config
(SPEC §9). One `edge.toml` drives both platforms; this tool renders the
per-platform configs deterministically.

```
edge-cli generate [--edge-toml edge.toml] [--out-dir .]
                  [--compatibility-date 2025-08-01]
edge-cli check    [--edge-toml edge.toml] [--fastly-toml fastly.toml]
```

## generate

Writes `wrangler.toml` (Cloudflare) and `fastly.toml` (Fastly) from
`edge.toml`:

- **fastly.toml** — deploy-time `[setup]`: `backends` (keyed by backend name,
  with `target`/`override_host`/`use_ssl` per origin), `kv_store` /
  `config_store` / `secret_store` from `[stores]`, and `[logging]` endpoints.
  An existing `[local_server]` section (Viceroy testing config, which is not
  derivable from `edge.toml`) is preserved verbatim, so `fastly compute
  deploy` configs and local-testing configs can live in one file.
- **wrangler.toml** — `name`, `main = "build/index.js"`, `compatibility_date`,
  and KV namespace bindings. KV namespaces omit `id`: wrangler
  auto-provisions the resource and writes the id back on the first deploy.
  Vars/secrets are left to platform configs (edge.toml only declares
  bindings). No fetch-permission config is emitted: wrangler (verified
  4.125.0) has no allowlist key — outbound `fetch` is allow-by-default on
  Workers.

Output is deterministic (origins sorted by alias) and idempotent:
regenerating over the previous output is a fixed point.

## check

Validates a deployed `fastly.toml` against `edge.toml` (D6: config drift is
the failure fail-closed resolution exists to catch):

- every origin's backend exists in `[setup.backends]`;
- `target` and `override_host` match the origin's URL host (Host-parity,
  D5.1) and `use_ssl` matches the URL scheme;
- `[stores]` bindings exist in `[setup]` with matching names.

Exits non-zero listing each problem found.

## Examples

```bash
# Generate both configs from the example service (deploy-ready).
edge-cli generate --edge-toml examples/hello-world/edge.toml --out-dir .

# Verify a deployed config still matches the manifest.
edge-cli check --edge-toml examples/hello-world/edge.toml \
               --fastly-toml examples/hello-world/fastly.toml
# -> OK: 0 origins, 2 store bindings — fastly.toml matches edge.toml
```
