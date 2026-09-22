#!/usr/bin/env bash
set -eo pipefail
set +u
BIN_DIR="$BIN_DIR"
set -u
if [[ -z "$BIN_DIR" ]]; then BIN_DIR=target/debug; fi

daemon="$BIN_DIR/semwrightd"
client="$BIN_DIR/computerctl"
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

[[trusted_mcp_stdio_upstreams]]
slug = "fixture"
program = "$fixture"
sha256 = "$sha"
expected_name = "semwright-fixture-upstream"
expected_version = "1.0.0"
request_timeout_ms = 5000
EOF
chmod 600 "$root/daemon.toml"

export XDG_RUNTIME_DIR="$root/runtime"
export XDG_STATE_HOME="$root/state"
socket="$root/runtime/semwright.sock"
session="$root/session.json"

"$daemon" --config "$root/daemon.toml" --socket "$socket" --log-format json \
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
assert len(rows) == 7
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
