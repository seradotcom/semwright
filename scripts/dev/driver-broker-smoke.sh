#!/usr/bin/env bash
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
BIN_DIR=${BIN_DIR:-"$ROOT/target/debug"}
DAEMON="$BIN_DIR/semwrightd"
CTL="$BIN_DIR/semwright"
FIXTURE="$BIN_DIR/semwright-driver-fixture"
SANDBOX="$BIN_DIR/semwright-sandbox"
for file in "$DAEMON" "$CTL" "$FIXTURE" "$SANDBOX"; do
  test -x "$file" || { echo "missing executable: $file" >&2; exit 2; }
done
command -v bwrap >/dev/null || { echo "bubblewrap unavailable" >&2; exit 5; }

TMP=$(mktemp -d)
DAEMON_PID=""
cleanup() {
  if [[ -n "$DAEMON_PID" ]]; then
    kill -TERM "$DAEMON_PID" 2>/dev/null || true
    wait "$DAEMON_PID" 2>/dev/null || true
  fi
  rm -rf "$TMP"
}
trap cleanup EXIT
chmod 700 "$TMP"
mkdir "$TMP/runtime" "$TMP/state" "$TMP/home"
chmod 700 "$TMP/runtime" "$TMP/state" "$TMP/home"
export XDG_RUNTIME_DIR="$TMP/runtime"
export XDG_STATE_HOME="$TMP/state"
export HOME="$TMP/home"
unset DISPLAY WAYLAND_DISPLAY DBUS_SESSION_BUS_ADDRESS || true
cp "$FIXTURE" "$TMP/driver"
chmod 700 "$TMP/driver"
sha=$(sha256sum "$TMP/driver" | awk '{print $1}')
version=$(python3 - "$ROOT/Cargo.toml" <<'PY'
import sys, tomllib
with open(sys.argv[1], "rb") as f:
    print(tomllib.load(f)["workspace"]["package"]["version"])
PY
)
cat > "$TMP/driver.json" <<JSON
{
  "manifest_version": 1,
  "protocol": 1,
  "id": "fixture",
  "version": "$version",
  "publisher": "semwright-tests",
  "executable": "$TMP/driver",
  "sha256": "$sha",
  "application": {
    "desktop_id": "org.semwright.DriverFixture",
    "process_names": [],
    "supported_versions": []
  },
  "transport": "stdio_v1",
  "mounts": [],
  "network": false,
  "request_timeout_ms": 5000,
  "interfaces": {
    "dynamic_capabilities": false,
    "cooperative_cancellation": false,
    "events": false,
    "health": true
  }
}
JSON
chmod 600 "$TMP/driver.json"

start_daemon() {
  local config=$1
  local label=$2
  SOCKET="$TMP/runtime/$label.sock"
  local log="$TMP/$label.log"
  "$DAEMON" --config "$config" --socket "$SOCKET" >"$log" 2>&1 &
  DAEMON_PID=$!
  for _ in $(seq 1 100); do
    if [[ -S "$SOCKET" ]]; then
      return 0
    fi
    if ! kill -0 "$DAEMON_PID" 2>/dev/null; then
      cat "$log" >&2
      return 1
    fi
    sleep 0.05
  done
  cat "$log" >&2
  return 1
}

stop_daemon() {
  kill -TERM "$DAEMON_PID"
  wait "$DAEMON_PID"
  DAEMON_PID=""
}
cat > "$TMP/deny.toml" <<TOML
drivers = ["$TMP/driver.json"]
driver_network = false

[policy]
profile = "observe"
TOML
chmod 600 "$TMP/deny.toml"
start_daemon "$TMP/deny.toml" deny
socket="$SOCKET"
session="$TMP/runtime/deny.session"
search=$("$CTL" --socket "$socket" --session-file "$session" --json capabilities search ping --provider driver:fixture)
set +e
denied=$("$CTL" --socket "$socket" --session-file "$session" --json execute driver.fixture.ping)
denied_rc=$?
set -e
[[ "$denied_rc" -eq 3 ]]
python3 - "$search" "$denied" <<'PY'
import json, sys
search, denied = map(json.loads, sys.argv[1:])
caps = search["data"]["capabilities"]
assert any(row["id"] == "driver.fixture.ping" for row in caps)
assert denied["ok"] is False
assert denied["error"]["code"] == "PolicyDenied"
print("driver discovery without grant: PASS")
print("driver policy denial: PASS")
PY
stop_daemon
cat > "$TMP/allow.toml" <<TOML
drivers = ["$TMP/driver.json"]
driver_network = false

[policy]
profile = "observe"
allow = ["driver:fixture"]
TOML
chmod 600 "$TMP/allow.toml"
start_daemon "$TMP/allow.toml" allow
socket="$SOCKET"
session="$TMP/runtime/allow.session"
allowed=$("$CTL" --socket "$socket" --session-file "$session" --json execute driver.fixture.ping)
providers=$("$CTL" --socket "$socket" --session-file "$session" --json capabilities search ping --provider driver:fixture)
python3 - "$allowed" "$providers" <<'PY'
import json, sys
allowed, providers = map(json.loads, sys.argv[1:])
assert allowed["ok"] is True
assert allowed["data"] == {"ok": True}
p = allowed["execution"]["provenance"]
assert p["provider"] == "driver:fixture"
assert p["source"] == "driver"
assert p["descriptor_sha256"]
assert p["provider_generation"] is not None
rows = providers["data"]["capabilities"]
assert len(rows) == 1 and rows[0]["id"] == "driver.fixture.ping"
assert rows[0]["provenance"]["provider"] == "driver:fixture"
print(json.dumps({
  "driver_broker_smoke": "PASS",
  "policy_denial": True,
  "policy_allow": True,
  "provider": p["provider"],
  "source": p["source"]
}, sort_keys=True))
PY
stop_daemon
