#!/usr/bin/env bash
# Run the M2 conformance suite under workerd (SPEC §12 M2 exit criteria:
# hello-world + T1–T4 pass under workerd).
#
# Prerequisites: worker-build on PATH, workerd on PATH, wasm32-unknown-unknown
# target installed.
#
# Builds both service cdylibs, runs worker-build (wasm-bindgen + esbuild shim),
# starts the T4 echo origin and both services under workerd, and asserts
# T1–T4 plus hello-world's routes.
set -euo pipefail

cd "$(dirname "$0")/../.."   # edge/
ROOT="$(pwd)"
CONF_DIR="$ROOT/tests/conformance"
HELLO_DIR="$ROOT/examples/hello-world"

ORIGIN_PORT=18080
CONF_PORT=8788
CONF_T6_PORT=8789
HELLO_PORT=8787

PIDS=()

cleanup() {
    for pid in "${PIDS[@]:-}"; do kill "$pid" 2>/dev/null || true; done
    pkill -f "workerd serve" 2>/dev/null || true
    pkill -f "origin.py" 2>/dev/null || true
}
trap cleanup EXIT

say() { printf '\n\033[1;34m== %s ==\033[0m\n' "$*"; }
fail() { printf '\033[1;31mFAIL:\033[0m %s\n' "$*"; exit 1; }

# --- build + worker-build ----------------------------------------------------
say "building conformance + hello-world for wasm32-unknown-unknown"
cargo build -p conformance --features cloudflare --target wasm32-unknown-unknown
cargo build -p hello-world --features cloudflare --target wasm32-unknown-unknown

say "worker-build (wasm-bindgen + esbuild shim)"
( cd "$CONF_DIR" && rm -rf build && worker-build --features cloudflare )
( cd "$HELLO_DIR" && rm -rf build && worker-build --features cloudflare )

# --- start echo origin + workerd services ------------------------------------
say "starting echo origin on :$ORIGIN_PORT"
setsid nohup python3 "$CONF_DIR/origin.py" > /tmp/edge-origin.log 2>&1 < /dev/null &
PIDS+=($!)
sleep 1

say "starting conformance service on :$CONF_PORT"
setsid nohup workerd serve "$CONF_DIR/workerd-conformance.capnp" \
    > /tmp/edge-conf-workerd.log 2>&1 < /dev/null &
PIDS+=($!)
sleep 2

# T6 instance: same worker, but globalOutbound points at a dead port so
# every fetch rejects (D16: CF surfaces rejections as Connection).
say "starting T6 conformance service on :$CONF_T6_PORT"
setsid nohup workerd serve "$CONF_DIR/workerd-conformance-t6.capnp" \
    > /tmp/edge-conf-t6-workerd.log 2>&1 < /dev/null &
PIDS+=($!)
sleep 2

say "starting hello-world on :$HELLO_PORT"
( cd "$HELLO_DIR" && setsid nohup workerd serve "$HELLO_DIR/workerd-hello-world.capnp" \
    > /tmp/edge-hello-workerd.log 2>&1 < /dev/null & )
sleep 2

assert_json() {
    # assert_json <expr> <expected> <curl-args...>
    local expr="$1"; shift
    local expected="$1"; shift
    local out got
    out="$(curl -s -m 10 "$@")" || fail "no response: curl -s $*"
    got="$(python3 -c "import json,sys; d=json.load(sys.stdin); print($expr)" <<<"$out")" \
        || fail "non-JSON response for: curl -s $* -> $out"
    [ "$got" = "$expected" ] || fail "expected $expr=$expected, got $got (body: $out)"
}

# --- T1: echo round-trip -------------------------------------------------------
say "T1 echo round-trip"
assert_json 'd["method"]' 'GET' -X GET --data 'some body' -H 'x-test: conformance' \
    "http://127.0.0.1:$CONF_PORT/t1?q=1&q=2"
assert_json 'd["path"]' '/t1' -X GET --data 'some body' -H 'x-test: conformance' \
    "http://127.0.0.1:$CONF_PORT/t1?q=1&q=2"
assert_json 'd["query"]' 'q=1&q=2' -X GET --data 'some body' -H 'x-test: conformance' \
    "http://127.0.0.1:$CONF_PORT/t1?q=1&q=2"
assert_json 'd["header_x_test"]' 'conformance' -X GET --data 'some body' -H 'x-test: conformance' \
    "http://127.0.0.1:$CONF_PORT/t1?q=1&q=2"
assert_json 'd["body"]' 'some body' -X GET --data 'some body' -H 'x-test: conformance' \
    "http://127.0.0.1:$CONF_PORT/t1?q=1&q=2"
echo "  T1 OK"

# --- T2: status + headers + UTF-8 body -----------------------------------------
say "T2 response"
out="$(curl -s -i -m 10 "http://127.0.0.1:$CONF_PORT/t2")"
grep -q "201 Created" <<<"$out" || fail "T2: expected 201, got: $(head -1 <<<"$out")"
grep -qi '^x-conformance: yes' <<<"$out" || fail "T2: missing x-conformance header"
grep -qi '^content-type: text/plain; charset=utf-8' <<<"$out" || fail "T2: wrong content-type"
grep -q $'h\xc3\xa9llo \xe4\xb8\x96\xe7\x95\x8c' <<<"$out" || fail "T2: wrong UTF-8 body"
echo "  T2 OK"

# --- T3: router params, query, 404 ---------------------------------------------
say "T3 router"
assert_json 'd["name"]' 'alice' "http://127.0.0.1:$CONF_PORT/t3/hello/alice?q=hi"
assert_json 'd["query_q"]' 'q=hi' "http://127.0.0.1:$CONF_PORT/t3/hello/alice?q=hi"
code="$(curl -s -m 10 -o /dev/null -w '%{http_code}' "http://127.0.0.1:$CONF_PORT/t3/nope")"
[ "$code" = "404" ] || fail "T3: unmatched route should 404, got $code"
echo "  T3 OK"

# --- T4: fetch to declared origin, Host parity (D5.1) ----------------------------
say "T4 fetch to declared origin"
assert_json 'd["host"]' 'api.example.com' "http://127.0.0.1:$CONF_PORT/t4"
assert_json 'd["path"]' '/t4-origin' "http://127.0.0.1:$CONF_PORT/t4"
assert_json 'd["query"]' 'from=t4' "http://127.0.0.1:$CONF_PORT/t4"
echo "  T4 OK"

# --- T5: undeclared host — fail-open on CF (documented, SPEC §7.5) ---------------
# Fastly fails closed (asserted in run.sh); on CF any URL is fetchable, so
# the same handler reaches the (echo) origin and reports ok.
say "T5 fetch to undeclared host (CF fail-open, documented)"
assert_json 'd["outcome"]' 'ok' "http://127.0.0.1:$CONF_PORT/t5"
echo "  T5 OK"

# --- T6: refused outbound surfaces as Connection (D16) ---------------------------
say "T6 fetch error surface"
assert_json 'd["outcome"]' 'error' "http://127.0.0.1:$CONF_T6_PORT/t6"
assert_json 'd["category"]' 'Connection' "http://127.0.0.1:$CONF_T6_PORT/t6"
echo "  T6 OK"

# --- T7: redirects are never auto-followed (D5.2) ---------------------------------
say "T7 redirect not followed"
out="$(curl -s -i -m 10 "http://127.0.0.1:$CONF_PORT/t7")"
grep -q "302" <<<"$out" || fail "T7: expected 302 passthrough, got: $(head -1 <<<"$out")"
grep -qi '^location: /t7-target' <<<"$out" || fail "T7: missing Location: /t7-target"
echo "  T7 OK"

# --- T11: sequential fetches ------------------------------------------------------
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

# --- hello-world smoke ------------------------------------------------------------
say "hello-world routes"
body="$(curl -s -m 10 "http://127.0.0.1:$HELLO_PORT/")"
grep -q "Hello, world!" <<<"$body" || fail "hello-world: expected greeting, got: $body"
body="$(curl -s -m 10 "http://127.0.0.1:$HELLO_PORT/hello/edge")"
grep -q "hello edge!" <<<"$body" || fail "hello-world: expected route greeting, got: $body"
code="$(curl -s -m 10 -o /dev/null -w '%{http_code}' "http://127.0.0.1:$HELLO_PORT/missing")"
[ "$code" = "404" ] || fail "hello-world: unknown route should 404, got $code"
echo "  hello-world OK"

say "ALL M2 CONFORMANCE CHECKS PASSED"
