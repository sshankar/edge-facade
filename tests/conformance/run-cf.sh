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

# --- T7: config vars/secrets (M4) ------------------------------------------------
say "T7 config vars/secrets"
assert_json 'd["greeting"]' 'Hello' "http://127.0.0.1:$CONF_PORT/t7"
assert_json 'd["api_key"]' 's3cret' "http://127.0.0.1:$CONF_PORT/t7"
assert_json 'd["missing"]' 'True' "http://127.0.0.1:$CONF_PORT/t7"
echo "  T7 OK"

# --- T8: KV put/get/delete round trip (M4) -----------------------------------------
say "T8 KV round trip"
assert_json 'd["text"]' 'hello 世界' "http://127.0.0.1:$CONF_PORT/t8"
assert_json 'd["missing"]' 'True' "http://127.0.0.1:$CONF_PORT/t8"
assert_json 'd["after_delete"]' 'True' "http://127.0.0.1:$CONF_PORT/t8"
assert_json 'd["binary_ok"]' 'True' "http://127.0.0.1:$CONF_PORT/t8"
echo "  T8 OK"

# --- r1: redirects are never auto-followed (D5.2) -----------------------------------
say "r1 redirect not followed"
out="$(curl -s -i -m 10 "http://127.0.0.1:$CONF_PORT/r1")"
grep -q "302" <<<"$out" || fail "r1: expected 302 passthrough, got: $(head -1 <<<"$out")"
grep -qi '^location: /t7-target' <<<"$out" || fail "r1: missing Location: /t7-target"
echo "  r1 OK"

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

# --- T12: streaming fetch + relay (M6, D21) ----------------------------------
# Same invariant as run.sh: first-chunk + relayed body == origin payload,
# with chunk boundaries platform-dependent.
say "T12 streaming relay"
full="$(curl -s -m 10 "http://127.0.0.1:$ORIGIN_PORT/t12-origin")"
hdr="$(mktemp)"
relay="$(curl -s -m 30 -D "$hdr" "http://127.0.0.1:$CONF_PORT/t12")"
first="$(grep -i '^x-t12-first-chunk:' "$hdr" | tr -d '\r' | awk '{print $2}')"
[ -n "$first" ] || fail "T12: missing x-t12-first-chunk header ($hdr)"
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
# The driver injects request.cf via workerd's `cf-blob` header (parsed into
# request.cf and stripped before the worker sees it) plus cf-connecting-ip.
# Unavailable fields (original header names, proxy classification, JA3/JA4)
# are None — never substituted.
say "P7 client metadata"
CF_BLOB='{"colo":"DFW","asn":12345,"asOrganization":"Test Org","country":"US","continent":"NA","city":"Austin","region":"Texas","regionCode":"TX","postalCode":"78701","metroCode":"635","latitude":"30.27","longitude":"-97.74","httpProtocol":"HTTP/2","tlsCipher":"AEAD-AES128-GCM-SHA256","tlsVersion":"TLSv1.3"}'
assert_json 'd["provider"]' 'Cloudflare' -H "cf-blob: $CF_BLOB" -H 'cf-connecting-ip: 203.0.113.7' "http://127.0.0.1:$CONF_PORT/p7"
assert_json 'd["client_ip"]' '203.0.113.7' -H "cf-blob: $CF_BLOB" -H 'cf-connecting-ip: 203.0.113.7' "http://127.0.0.1:$CONF_PORT/p7"
assert_json 'd["pop"]' 'DFW' -H "cf-blob: $CF_BLOB" -H 'cf-connecting-ip: 203.0.113.7' "http://127.0.0.1:$CONF_PORT/p7"
assert_json 'd["geo"]["country_code"]' 'US' -H "cf-blob: $CF_BLOB" "http://127.0.0.1:$CONF_PORT/p7"
assert_json 'd["geo"]["continent"]' 'NA' -H "cf-blob: $CF_BLOB" "http://127.0.0.1:$CONF_PORT/p7"
assert_json 'd["geo"]["city"]' 'Austin' -H "cf-blob: $CF_BLOB" "http://127.0.0.1:$CONF_PORT/p7"
assert_json 'd["geo"]["region_code"]' 'TX' -H "cf-blob: $CF_BLOB" "http://127.0.0.1:$CONF_PORT/p7"
assert_json 'd["geo"]["postal_code"]' '78701' -H "cf-blob: $CF_BLOB" "http://127.0.0.1:$CONF_PORT/p7"
assert_json 'd["geo"]["metro_code"]' '635' -H "cf-blob: $CF_BLOB" "http://127.0.0.1:$CONF_PORT/p7"
assert_json 'd["geo"]["latitude"]' '30.27' -H "cf-blob: $CF_BLOB" "http://127.0.0.1:$CONF_PORT/p7"
assert_json 'd["geo"]["longitude"]' '-97.74' -H "cf-blob: $CF_BLOB" "http://127.0.0.1:$CONF_PORT/p7"
assert_json 'd["network"]["asn"]' '12345' -H "cf-blob: $CF_BLOB" "http://127.0.0.1:$CONF_PORT/p7"
assert_json 'd["network"]["as_organization"]' 'Test Org' -H "cf-blob: $CF_BLOB" "http://127.0.0.1:$CONF_PORT/p7"
assert_json 'd["tls"]["protocol"]' 'TLSv1.3' -H "cf-blob: $CF_BLOB" "http://127.0.0.1:$CONF_PORT/p7"
assert_json 'd["tls"]["cipher"]' 'AEAD-AES128-GCM-SHA256' -H "cf-blob: $CF_BLOB" "http://127.0.0.1:$CONF_PORT/p7"
out="$(curl -s -H "cf-blob: $CF_BLOB" "http://127.0.0.1:$CONF_PORT/p7")"
python3 - "$out" <<'PY' || fail "P7: unavailable fields must be None on Cloudflare"
import json, sys
d = json.loads(sys.argv[1])
assert d["original_header_names"] is None, d["original_header_names"]
assert d["network"]["proxy_type"] is None and d["network"]["proxy_description"] is None, d["network"]
assert d["tls"]["ja3"] is None and d["tls"]["ja4"] is None, d["tls"]
PY
# Without cf-blob / cf-connecting-ip, request.cf is absent -> all None.
out="$(curl -s "http://127.0.0.1:$CONF_PORT/p7")"
python3 - "$out" <<'PY' || fail "P7: absent cf must yield all-None metadata"
import json, sys
d = json.loads(sys.argv[1])
assert d["client_ip"] is None and d["pop"] is None, (d["client_ip"], d["pop"])
assert d["geo"]["country_code"] is None and d["network"]["asn"] is None, (d["geo"], d["network"])
PY
echo "  P7 OK"

# --- P8: original header names (None, never reconstructed) -------------------
say "P8 original header names"
out="$(curl -s -H 'X-Mixed-Case: v' "http://127.0.0.1:$CONF_PORT/p8")"
python3 - "$out" <<'PY' || fail "P8: Cloudflare never reconstructs original header names"
import json, sys
d = json.loads(sys.argv[1])
assert d["original_header_names"] is None, d["original_header_names"]
assert d["header_count"] >= 2, d
assert d["saw_original_case"] is None, d["saw_original_case"]
PY
echo "  P8 OK"

# --- P9: log fields on success and synthetic error (M11) ---------------------
# On Cloudflare the finalized fields ride in the x-edge-log-fields control
# response header (the boundary record); success and error legs must carry
# the same logical map.
say "P9 log fields (success + synthetic error)"
out="$(curl -s -i "http://127.0.0.1:$CONF_PORT/p9")"
grep -qi '^x-edge-log-fields: {\"origin\":\"api-a\",\"request_id\":\"req-123\"}' <<<"$out" \
    || fail "P9: success leg control header missing/incorrect: $(grep -i 'x-edge-log-fields' <<<"$out")"
out="$(curl -s -i "http://127.0.0.1:$CONF_PORT/p9-error")"
grep -q '^HTTP/1.1 500' <<<"$out" || fail "P9: synthetic error should be 500"
grep -qi '^x-edge-log-fields: {\"origin\":\"api-a\",\"request_id\":\"req-123\"}' <<<"$out" \
    || fail "P9: error leg control header missing/incorrect: $(grep -i 'x-edge-log-fields' <<<"$out")"
echo "  P9 OK"

# --- P10: origin control-field injection -------------------------------------
# The injected value must be stripped; the header carries only the facade's
# finalized fields.
say "P10 control-field injection"
out="$(curl -s -i "http://127.0.0.1:$CONF_PORT/p10")"
grep -qi '^x-edge-log-fields: {\"tenant\":\"t1\"}' <<<"$out" \
    || fail "P10: control header must carry only the facade's fields: $(grep -i 'x-edge-log-fields' <<<"$out")"
grep -qi 'injected=origin-value' <<<"$out" && fail "P10: injected value leaked to the client"
echo "  P10 OK"

# --- P11: budget enforcement -------------------------------------------------
# 20 fields x 303 bytes exceed the aggregate budget; the header must carry
# exactly the deterministic retained set (the 13 newest, f07..=f19).
say "P11 log-field budget"
out="$(curl -s -D - "http://127.0.0.1:$CONF_PORT/p11")"
python3 - "$out" <<'PY' || fail "P11: retained set mismatch in the control header"
import json, re, sys
header = re.search(r"(?im)^x-edge-log-fields: (.+?)\r?$", sys.argv[1])
assert header, "missing x-edge-log-fields header"
fields = json.loads(header.group(1))
expected = {f"f{i:02}": "x" * 300 for i in range(7, 20)}
assert fields == expected, (sorted(fields), sorted(expected))
PY
echo "  P11 OK"

# --- hello-world smoke ------------------------------------------------------------
say "hello-world routes"
body="$(curl -s -m 10 "http://127.0.0.1:$HELLO_PORT/")"
grep -q "Hi, world!" <<<"$body" || fail "hello-world: expected config-driven greeting, got: $body"
body="$(curl -s -m 10 "http://127.0.0.1:$HELLO_PORT/hello/edge")"
grep -q "hello edge!" <<<"$body" || fail "hello-world: expected route greeting, got: $body"
code="$(curl -s -m 10 -o /dev/null -w '%{http_code}' "http://127.0.0.1:$HELLO_PORT/missing")"
[ "$code" = "404" ] || fail "hello-world: unknown route should 404, got $code"
echo "  hello-world OK"

say "ALL M2 CONFORMANCE CHECKS PASSED"
