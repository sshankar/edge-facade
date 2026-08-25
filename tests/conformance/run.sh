#!/usr/bin/env bash
# Run the M1 conformance suite under Viceroy (SPEC §12 M1 exit criteria:
# hello-world + T1–T4 pass under Viceroy).
#
# Prerequisites: viceroy on PATH, wasm32-wasip1 target installed.
#
# Builds the conformance and hello-world wasm modules, starts a local echo
# origin for T4, runs both services under Viceroy, and asserts the
# T1–T4 responses plus hello-world's routes.
set -euo pipefail

cd "$(dirname "$0")/../.."   # edge/
ROOT="$(pwd)"
CONF_DIR="$ROOT/tests/conformance"

ORIGIN_PORT=18080
CONF_PORT=7676
HELLO_PORT=7677

WASM_DIR="$ROOT/target/wasm32-wasip1/debug"
CONF_WASM="$WASM_DIR/conformance-fastly.wasm"
HELLO_WASM="$WASM_DIR/hello-world.wasm"

PIDS=()

cleanup() {
    for pid in "${PIDS[@]:-}"; do kill "$pid" 2>/dev/null || true; done
    wait 2>/dev/null || true
}
trap cleanup EXIT

say() { printf '\n\033[1;34m== %s ==\033[0m\n' "$*"; }
fail() { printf '\033[1;31mFAIL:\033[0m %s\n' "$*"; exit 1; }

# --- build -----------------------------------------------------------------
say "building conformance + hello-world for wasm32-wasip1"
cargo build -p conformance --features fastly --target wasm32-wasip1
cargo build -p hello-world --features fastly --target wasm32-wasip1

# --- start echo origin ------------------------------------------------------
say "starting echo origin on :$ORIGIN_PORT"
python3 "$CONF_DIR/origin.py" &
PIDS+=($!)
sleep 1

# --- start Viceroy services -------------------------------------------------
say "starting conformance service on :$CONF_PORT"
# stdout is captured so the P9–P11 log-endpoint records can be asserted
# (Viceroy routes endpoint writes to the process stdout).
viceroy serve -C "$CONF_DIR/fastly.toml" --addr "127.0.0.1:$CONF_PORT" "$CONF_WASM" \
    > "$ROOT/target/viceroy-conformance.log" 2>&1 &
PIDS+=($!)
sleep 1

say "starting hello-world on :$HELLO_PORT"
viceroy serve -C "$ROOT/examples/hello-world/fastly.toml" --addr "127.0.0.1:$HELLO_PORT" "$HELLO_WASM" &
PIDS+=($!)
sleep 1

assert_json() {
    # assert_json <curl-args...> <jq-style python expr> <expected>
    local expr="$1"; shift
    local expected="$1"; shift
    local out
    out="$(curl -s "$@")"
    local got
    got="$(python3 -c "import json,sys; d=json.load(sys.stdin); print($expr)" <<<"$out")" \
        || fail "non-JSON response for: curl -s $* -> $out"
    [ "$got" = "$expected" ] || fail "expected $expr=$expected, got $got (body: $out)"
}

# --- T1: echo round-trip -----------------------------------------------------
say "T1 echo round-trip"
assert_json 'd["method"]' 'GET' -X GET --data 'some body' -H 'x-test: conformance' \
    "http://127.0.0.1:$CONF_PORT/t1?q=1&q=2"
assert_json 'd["path"]' '/t1' -X GET --data 'some body' -H 'x-test: conformance' \
    "http://127.0.0.1:$CONF_PORT/t1?q=1&q=2"
assert_json 'd["query"]' 'q=1&q=2' -X GET --data 'some body' -H 'x-test: conformance' \
    "http://127.0.0.1:$CONF_PORT/t1?q=1&q=2"
assert_json 'd["header_x_test"]' 'conformance' -X GET --data 'some body' \
    -H 'x-test: conformance' "http://127.0.0.1:$CONF_PORT/t1?q=1&q=2"
assert_json 'd["body"]' 'some body' -X GET --data 'some body' -H 'x-test: conformance' \
    "http://127.0.0.1:$CONF_PORT/t1?q=1&q=2"
echo "  T1 OK"

# --- T2: status + headers + UTF-8 body --------------------------------------
say "T2 response"
out="$(curl -s -i "http://127.0.0.1:$CONF_PORT/t2")"
grep -q "201 Created" <<<"$out" || fail "T2: expected 201, got: $(head -1 <<<"$out")"
grep -qi '^x-conformance: yes' <<<"$out" || fail "T2: missing x-conformance header"
grep -qi '^content-type: text/plain; charset=utf-8' <<<"$out" || fail "T2: wrong content-type"
grep -q $'h\xc3\xa9llo \xe4\xb8\x96\xe7\x95\x8c' <<<"$out" || fail "T2: wrong UTF-8 body"
echo "  T2 OK"

# --- T3: router params, query, 404 ------------------------------------------
say "T3 router"
assert_json 'd["name"]' 'alice' "http://127.0.0.1:$CONF_PORT/t3/hello/alice?q=hi"
assert_json 'd["query_q"]' 'q=hi' "http://127.0.0.1:$CONF_PORT/t3/hello/alice?q=hi"
code="$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$CONF_PORT/t3/nope")"
[ "$code" = "404" ] || fail "T3: unmatched route should 404, got $code"
echo "  T3 OK"

# --- T4: fetch to declared origin, Host parity (D5.1) ------------------------
say "T4 fetch to declared origin"
assert_json 'd["host"]' 'api.example.com' "http://127.0.0.1:$CONF_PORT/t4"
assert_json 'd["path"]' '/t4-origin' "http://127.0.0.1:$CONF_PORT/t4"
assert_json 'd["query"]' 'from=t4' "http://127.0.0.1:$CONF_PORT/t4"
echo "  T4 OK"

# --- T5: undeclared host fails closed (D4) ------------------------------------
say "T5 fetch to undeclared host"
assert_json 'd["outcome"]' 'error' "http://127.0.0.1:$CONF_PORT/t5"
assert_json 'd["category"]' 'UnresolvedBackend' "http://127.0.0.1:$CONF_PORT/t5"
assert_json 'd["host"]' 'undeclared.example.com' "http://127.0.0.1:$CONF_PORT/t5"
echo "  T5 OK"

# --- T6: refused origin surfaces as Connection (error-surface parity) ---------
say "T6 fetch error surface"
assert_json 'd["outcome"]' 'error' "http://127.0.0.1:$CONF_PORT/t6"
assert_json 'd["category"]' 'Connection' "http://127.0.0.1:$CONF_PORT/t6"
echo "  T6 OK"

# --- T7: config vars/secrets (M4) -------------------------------------------
say "T7 config vars/secrets"
assert_json 'd["greeting"]' 'Hello' "http://127.0.0.1:$CONF_PORT/t7"
assert_json 'd["api_key"]' 's3cret' "http://127.0.0.1:$CONF_PORT/t7"
assert_json 'd["missing"]' 'True' "http://127.0.0.1:$CONF_PORT/t7"
echo "  T7 OK"

# --- T8: KV put/get/delete round trip (M4) ------------------------------------
say "T8 KV round trip"
assert_json 'd["text"]' 'hello 世界' "http://127.0.0.1:$CONF_PORT/t8"
assert_json 'd["missing"]' 'True' "http://127.0.0.1:$CONF_PORT/t8"
assert_json 'd["after_delete"]' 'True' "http://127.0.0.1:$CONF_PORT/t8"
assert_json 'd["binary_ok"]' 'True' "http://127.0.0.1:$CONF_PORT/t8"
echo "  T8 OK"

# --- r1: redirects are never auto-followed (D5.2) ------------------------------
say "r1 redirect not followed"
out="$(curl -s -i -m 10 "http://127.0.0.1:$CONF_PORT/r1")"
grep -q "302" <<<"$out" || fail "r1: expected 302 passthrough, got: $(head -1 <<<"$out")"
grep -qi '^location: /t7-target' <<<"$out" || fail "r1: missing Location: /t7-target"
echo "  r1 OK"

# --- T11: sequential fetches (D3 executor contract) ---------------------------
say "T11 sequential fetches"
out="$(curl -s -m 10 "http://127.0.0.1:$CONF_PORT/t11")"
python3 -c '
import json, sys
d = json.load(sys.stdin)
first = json.loads(d["first"])
second = json.loads(d["second"])
assert first["host"] == "api.example.com" and first["path"] == "/t11-first", first
assert second["host"] == "api.example.com" and second["path"] == "/t11-second", second
' <<<"$out" || fail "T11: sequential fetch assertions failed (body: $out)"
echo "  T11 OK"

# --- T12: streaming fetch + relay (M6, D21) ----------------------------------
# The handler reads exactly one chunk of the origin payload, then relays the
# remainder as a stream. Chunk boundaries are platform-dependent, so drivers
# assert the invariant that holds everywhere: first-chunk (x-t12-first-chunk)
# + relayed body == the origin's full payload.
say "T12 streaming relay"
full="$(curl -s -m 10 "http://127.0.0.1:$ORIGIN_PORT/t12-origin")"
hdr="$(mktemp)"
relay="$(curl -s -m 30 -D "$hdr" "http://127.0.0.1:$CONF_PORT/t12")"
first="$(grep -i '^x-t12-first-chunk:' "$hdr" | tr -d '\r' | awk '{print $2}')"
[ -n "$first" ] || fail "T12: missing x-t12-first-chunk header ($hdr)"
# Streaming proof: a streamed relay has no Content-Length — Fastly sends
# transfer-encoding: chunked (SPEC D21). A buffered relay would send
# Content-Length.
grep -qi '^transfer-encoding: chunked' "$hdr" \
    || fail "T12: expected chunked (streamed) relay, got: $(head -6 "$hdr")"
python3 - "$full" "$first" "$relay" <<'PY' || fail "T12: streaming invariant broken"; echo "  T12 OK"
import sys
full, first, relay = sys.argv[1], int(sys.argv[2]), sys.argv[3]
assert first > 0, f"first chunk empty: {first}"
assert first <= len(full), f"first chunk longer than payload: {first}"
assert full[first:] == relay, (
    f"relay mismatch: full={len(full)} first={first} relay={len(relay)}"
)
PY

# --- P7: client metadata (M10) ----------------------------------------------
# Under Viceroy the client IP is the peer address (127.0.0.1), the POP comes
# from FASTLY_POP=XXX, geo/network come from the [local_server.geolocation]
# fixture, original header names come from the downstream original-header
# API (lowercased by Viceroy's hyper stack — original spelling is preserved
# on real Fastly), and TLS metadata is absent (None).
say "P7 client metadata"
assert_json 'd["provider"]' 'Fastly' "http://127.0.0.1:$CONF_PORT/p7"
assert_json 'd["client_ip"]' '127.0.0.1' "http://127.0.0.1:$CONF_PORT/p7"
assert_json 'd["pop"]' 'XXX' "http://127.0.0.1:$CONF_PORT/p7"
assert_json 'd["geo"]["country_code"]' 'US' "http://127.0.0.1:$CONF_PORT/p7"
assert_json 'd["geo"]["continent"]' 'NA' "http://127.0.0.1:$CONF_PORT/p7"
assert_json 'd["geo"]["city"]' 'Austin' "http://127.0.0.1:$CONF_PORT/p7"
assert_json 'd["geo"]["region_code"]' 'Texas' "http://127.0.0.1:$CONF_PORT/p7"
assert_json 'd["geo"]["postal_code"]' '78701' "http://127.0.0.1:$CONF_PORT/p7"
assert_json 'd["geo"]["metro_code"]' '635' "http://127.0.0.1:$CONF_PORT/p7"
assert_json 'd["geo"]["latitude"]' '30.27' "http://127.0.0.1:$CONF_PORT/p7"
assert_json 'd["geo"]["longitude"]' '-97.74' "http://127.0.0.1:$CONF_PORT/p7"
assert_json 'd["network"]["asn"]' '64512' "http://127.0.0.1:$CONF_PORT/p7"
assert_json 'd["network"]["as_organization"]' 'Example Org' "http://127.0.0.1:$CONF_PORT/p7"
assert_json 'd["network"]["proxy_type"]' 'Hosting' "http://127.0.0.1:$CONF_PORT/p7"
assert_json 'd["network"]["proxy_description"]' 'Cloud' "http://127.0.0.1:$CONF_PORT/p7"
out="$(curl -s "http://127.0.0.1:$CONF_PORT/p7")"
python3 - "$out" <<'PY' || fail "P7: TLS metadata should be None under Viceroy"
import json, sys
d = json.loads(sys.argv[1])
assert d["tls"] == {"protocol": None, "cipher": None, "ja3": None, "ja4": None, "ciphers_sha1": None, "extensions_sha1": None}, d["tls"]
assert d["original_header_names"] is not None and len(d["original_header_names"]) > 0, d["original_header_names"]
PY
echo "  P7 OK"

# --- P8: original header names -----------------------------------------------
# The original-header API is available on Fastly: the received names (in
# Viceroy's case lowercased by hyper) must be reported — never `None` — and
# the injected mixed-case header is present in its runtime-provided spelling.
say "P8 original header names"
out="$(curl -s -H 'X-Mixed-Case: v' "http://127.0.0.1:$CONF_PORT/p8")"
python3 - "$out" <<'PY' || fail "P8: original_header_names must be a non-empty list on Fastly"
import json, sys
d = json.loads(sys.argv[1])
assert d["original_header_names"] is not None, d
assert "x-mixed-case" in d["original_header_names"], d["original_header_names"]
assert d["header_count"] >= 2, d
PY
echo "  P8 OK"

# --- P9: log fields on success and synthetic error (M11) ---------------------
# The structured record is emitted to the configured log endpoint at
# finalization for both outcomes; the logical map is identical.
say "P9 log fields (success + synthetic error)"
curl -s -o /dev/null "http://127.0.0.1:$CONF_PORT/p9"
code="$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$CONF_PORT/p9-error")"
[ "$code" = "500" ] || fail "P9: synthetic error should be 500, got $code"
records="$(grep -c 'conformance_logging :: {"fields":{"origin":"api-a","request_id":"req-123"}}' \
    "$ROOT/target/viceroy-conformance.log" || true)"
[ "$records" -ge 2 ] || fail "P9: expected >= 2 finalized records (success + error), found $records"
echo "  P9 OK"

# --- P10: origin control-field injection -------------------------------------
# The injected x-edge-log-fields value must be stripped from the client
# response (with a diagnostic); the finalized record carries only the
# handler's own fields.
say "P10 control-field injection"
out="$(curl -s -i "http://127.0.0.1:$CONF_PORT/p10")"
grep -qi '^x-edge-log-fields:' <<<"$out" \
    && fail "P10: control header must not reach the client on Fastly"
grep -q "conformance_logging :: {\"fields\":{\"tenant\":\"t1\"}}" "$ROOT/target/viceroy-conformance.log" \
    || fail "P10: finalized record missing from the log endpoint"
grep -q "stripped client-visible logging control header" "$ROOT/target/viceroy-conformance.log" \
    || fail "P10: strip diagnostic missing from the log endpoint"
echo "  P10 OK"

# --- P11: budget enforcement -------------------------------------------------
# 20 fields x 303 bytes exceed the 4096-byte aggregate budget: the retained
# set is deterministic (the 13 newest, f07..=f19) and no control data
# reaches the client response.
say "P11 log-field budget"
out="$(curl -s -i "http://127.0.0.1:$CONF_PORT/p11")"
grep -qi '^x-edge-log-fields:' <<<"$out" \
    && fail "P11: control header must not reach the client on Fastly"
python3 - "$ROOT/target/viceroy-conformance.log" <<'PY' || fail "P11: retained set mismatch in the log record"
import json, re, sys
log = open(sys.argv[1]).read()
# Find the finalization record carrying budget-test fields (the log also
# contains the earlier P9/P10 records).
matches = re.findall(r"conformance_logging :: (\{\"fields\":\{.*\}\})", log)
assert matches, "no finalized record found"
candidates = [json.loads(m)["fields"] for m in matches]
budget = [f for f in candidates if any(k.startswith("f") for k in f)]
assert budget, "no budget record found"
expected = {f"f{i:02}": "x" * 300 for i in range(7, 20)}
assert budget[-1] == expected, (sorted(budget[-1]), sorted(expected))
PY
grep -q "aggregate budget" "$ROOT/target/viceroy-conformance.log" \
    || fail "P11: budget diagnostic missing from the log endpoint"
echo "  P11 OK"

# --- hello-world smoke -------------------------------------------------------
say "hello-world routes"
body="$(curl -s "http://127.0.0.1:$HELLO_PORT/")"
grep -q "Hi, world!" <<<"$body" || fail "hello-world: expected config-driven greeting, got: $body"
body="$(curl -s "http://127.0.0.1:$HELLO_PORT/hello/edge")"
grep -q "hello edge!" <<<"$body" || fail "hello-world: expected route greeting, got: $body"
code="$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$HELLO_PORT/missing")"
[ "$code" = "404" ] || fail "hello-world: unknown route should 404, got $code"
echo "  hello-world OK"

say "ALL M1 CONFORMANCE CHECKS PASSED"
