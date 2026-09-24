#!/usr/bin/env bash
set -eo pipefail
BIN_DIR=${BIN_DIR:-target/debug}
set -u

daemon="$BIN_DIR/semwrightd"
client="$BIN_DIR/semwright"
fixture="$BIN_DIR/semwright-mcp-fixture"

for binary in "$daemon" "$client" "$fixture"; do
  test -x "$binary" || {
    echo "missing executable: $binary" >&2
    exit 2
  }
done

root="$(mktemp -d)"
daemon_pid=""
child_pid=""
cleanup() {
  if [[ -n "$daemon_pid" ]] && kill -0 "$daemon_pid" 2>/dev/null; then
    kill -TERM "$daemon_pid" 2>/dev/null || true
    for _ in $(seq 1 50); do
      kill -0 "$daemon_pid" 2>/dev/null || break
      sleep 0.1
    done
  fi
  if [[ -n "$child_pid" ]] && kill -0 "$child_pid" 2>/dev/null; then
    echo "federated MCP child survived broker shutdown" >&2
    kill -KILL "$child_pid" 2>/dev/null || true
    rm -rf "$root"
    exit 1
  fi
  rm -rf "$root"
}
trap cleanup EXIT INT TERM

chmod 700 "$root"
mkdir -m 700 "$root/runtime" "$root/state"
fixture="$(realpath "$fixture")"
chmod 755 "$fixture"
sha="$(sha256sum "$fixture" | awk '{print $1}')"

cat >"$root/daemon.toml" <<EOF
[policy]
profile = "observe"
allow = ["external-mcp:fixture"]
EOF
chmod 600 "$root/daemon.toml"

export XDG_RUNTIME_DIR="$root/runtime"
export XDG_STATE_HOME="$root/state"
registry="$root/mcp-upstreams.toml"
socket="$root/runtime/semwright.sock"
session="$root/session.json"

"$client" --json --dry-run --mcp-upstreams "$registry" mcp upstream add fixture "$fixture" \
  --expected-name semwright-fixture-upstream --expected-version 1.0.0 \
  --request-timeout-ms 5000 >"$root/upstream-add-dry.json"
test ! -e "$registry" || {
  echo "dry-run unexpectedly created the owner registry" >&2
  exit 1
}

"$client" --json --mcp-upstreams "$registry" mcp upstream add fixture "$fixture" \
  --expected-name semwright-fixture-upstream --expected-version 1.0.0 \
  --request-timeout-ms 5000 >"$root/upstream-add.json"
"$client" --json --mcp-upstreams "$registry" mcp upstream list >"$root/upstream-list.json"
"$client" --json --mcp-upstreams "$registry" mcp upstream inspect fixture >"$root/upstream-inspect.json"
"$client" --json --mcp-upstreams "$registry" mcp upstream disable fixture >"$root/upstream-disable.json"
"$client" --json --mcp-upstreams "$registry" mcp upstream enable fixture >"$root/upstream-enable.json"
"$client" --json --dry-run --mcp-upstreams "$registry" mcp upstream doctor fixture >"$root/upstream-doctor-dry.json"
"$client" --json --mcp-upstreams "$registry" mcp upstream doctor fixture >"$root/upstream-doctor.json"

python3 - "$root/upstream-add-dry.json" "$root/upstream-add.json" "$root/upstream-list.json" "$root/upstream-inspect.json" \
  "$root/upstream-disable.json" "$root/upstream-enable.json" "$root/upstream-doctor-dry.json" \
  "$root/upstream-doctor.json" "$sha" <<'PY'
import json
import sys

dry_add, add, listing, inspect, disable, enable, dry_doctor, doctor = [
    json.load(open(path, encoding="utf-8")) for path in sys.argv[1:9]
]
sha = sys.argv[9]
assert dry_add["dry_run"] is True and dry_add["saved"] is False
assert dry_add["policy_grants_changed"] is False
assert add["saved"] is True and add["policy_grants_changed"] is False
assert add["upstream"]["sha256"] == sha
assert listing["policy_grants_changed"] is False
assert [row["slug"] for row in listing["upstreams"]] == ["fixture"]
assert inspect["upstream"]["required_policy_scope"] == "external-mcp:fixture"
assert disable["enabled"] is False and disable["restart_required"] is True
assert enable["enabled"] is True and enable["restart_required"] is True
assert dry_doctor["dry_run"] is True and dry_doctor["launched"] is False
assert dry_doctor["executable_valid"] is True
assert doctor["healthy"] is True and doctor["launched"] is True
assert doctor["provider"] == "external-mcp:fixture"
assert doctor["capabilities"] == 8
print("federation owner management smoke: PASS")
PY

# Definition management and policy authority stay separate.
if grep -q "policy" "$registry"; then
  echo "upstream registry unexpectedly contains policy authority" >&2
  exit 1
fi

"$daemon" --config "$root/daemon.toml" --mcp-upstreams "$registry" --socket "$socket" --log-format json \
  >"$root/daemon.log" 2>&1 &
daemon_pid=$!

for _ in $(seq 1 100); do
  [[ -S "$socket" ]] && break
  kill -0 "$daemon_pid" 2>/dev/null || {
    cat "$root/daemon.log" >&2
    exit 1
  }
  sleep 0.05
done
[[ -S "$socket" ]] || {
  cat "$root/daemon.log" >&2
  echo "daemon socket was not created" >&2
  exit 1
}

for _ in $(seq 1 100); do
  child_pid="$(pgrep -P "$daemon_pid" -f 'semwright-mcp-fixture' | head -1 || true)"
  [[ -n "$child_pid" ]] && break
  sleep 0.02
done
[[ -n "$child_pid" ]] || {
  cat "$root/daemon.log" >&2
  echo "federated MCP child was not observed" >&2
  exit 1
}

"$client" --json --socket "$socket" --session-file "$session" doctor >"$root/doctor.json"
"$client" --json --socket "$socket" --session-file "$session" \
  capabilities search --provider external-mcp:fixture --limit 20 >"$root/search.json"

python3 - "$root/doctor.json" "$root/search.json" "$sha" <<'PY'
import json
import sys

doctor = json.load(open(sys.argv[1], encoding="utf-8"))
search = json.load(open(sys.argv[2], encoding="utf-8"))
sha = sys.argv[3]

assert doctor["ok"] is True
providers = doctor["data"]["providers"]["providers"]
provider = next(p for p in providers if p["identity"]["id"] == "external-mcp:fixture")
assert provider["connected"] is True
assert provider["identity"]["kind"] == "external_mcp"
assert provider["identity"]["origin"] == f"trusted-stdio-sha256:{sha}"
assert provider["interfaces"]["dynamic_capabilities"] is True

assert search["ok"] is True
rows = search["data"]["capabilities"]
assert len(rows) == 8
assert all(row["provenance"]["provider"] == "external-mcp:fixture" for row in rows)
assert all(row["provenance"]["source"] == "external_mcp" for row in rows)
assert all(row["provenance"]["untrusted_metadata"] is True for row in rows)
assert all(row["risk"] == "privilege_sensitive" for row in rows)
assert any(
    "IGNORE ALL PREVIOUS INSTRUCTIONS" in row["summary"]
    and row["provenance"]["untrusted_metadata"] is True
    for row in rows
)
print("federation daemon smoke: PASS")
PY

kill -TERM "$daemon_pid"
wait "$daemon_pid"
daemon_pid=""

for _ in $(seq 1 50); do
  kill -0 "$child_pid" 2>/dev/null || break
  sleep 0.1
done
kill -0 "$child_pid" 2>/dev/null && {
  echo "federated MCP child survived broker shutdown" >&2
  exit 1
}
child_pid=""


"$client" --json --mcp-upstreams "$registry" mcp upstream remove fixture >"$root/upstream-remove.json"
"$client" --json --mcp-upstreams "$registry" mcp upstream list >"$root/upstream-empty.json"
python3 - "$root/upstream-remove.json" "$root/upstream-empty.json" <<'PY'
import json
import sys
removed = json.load(open(sys.argv[1], encoding="utf-8"))
empty = json.load(open(sys.argv[2], encoding="utf-8"))
assert removed["removed"] is True and removed["policy_grants_changed"] is False
assert empty["upstreams"] == []
print("federation owner removal smoke: PASS")
PY
