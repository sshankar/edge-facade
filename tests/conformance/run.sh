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
viceroy serve -C "$CONF_DIR/fastly.toml" --addr "127.0.0.1:$CONF_PORT" "$CONF_WASM" &
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

# --- T7: redirects are never auto-followed (D5.2) -----------------------------
say "T7 redirect not followed"
out="$(curl -s -i -m 10 "http://127.0.0.1:$CONF_PORT/t7")"
grep -q "302" <<<"$out" || fail "T7: expected 302 passthrough, got: $(head -1 <<<"$out")"
grep -qi '^location: /t7-target' <<<"$out" || fail "T7: missing Location: /t7-target"
echo "  T7 OK"

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

# --- hello-world smoke -------------------------------------------------------
say "hello-world routes"
body="$(curl -s "http://127.0.0.1:$HELLO_PORT/")"
grep -q "Hello, world!" <<<"$body" || fail "hello-world: expected greeting, got: $body"
body="$(curl -s "http://127.0.0.1:$HELLO_PORT/hello/edge")"
grep -q "hello edge!" <<<"$body" || fail "hello-world: expected route greeting, got: $body"
code="$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$HELLO_PORT/missing")"
[ "$code" = "404" ] || fail "hello-world: unknown route should 404, got $code"
echo "  hello-world OK"

say "ALL M1 CONFORMANCE CHECKS PASSED"
