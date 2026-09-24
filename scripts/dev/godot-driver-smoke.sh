#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
BIN_DIR=${BIN_DIR:-"$ROOT/target/debug"}
DAEMON="$BIN_DIR/semwrightd"
CTL="$BIN_DIR/semwright"
DRIVER="$BIN_DIR/semwright-godot-driver"
SANDBOX="$BIN_DIR/semwright-sandbox"
FAKE="$BIN_DIR/examples/godot-fake-editor"

for file in "$DAEMON" "$CTL" "$DRIVER" "$SANDBOX" "$FAKE"; do
  test -x "$file" || { echo "missing executable: $file" >&2; exit 2; }
done
for command in bwrap python3 sha256sum; do
  command -v "$command" >/dev/null || { echo "missing dependency: $command" >&2; exit 5; }
done

TMP=$(mktemp -d)
DAEMON_PID=""
FAKE_PID=""
cleanup() {
  if [[ -n "$DAEMON_PID" ]]; then
    kill -TERM "$DAEMON_PID" 2>/dev/null || true
    wait "$DAEMON_PID" 2>/dev/null || true
  fi
  if [[ -n "$FAKE_PID" ]]; then
    kill -TERM "$FAKE_PID" 2>/dev/null || true
    wait "$FAKE_PID" 2>/dev/null || true
  fi
  rm -rf "$TMP"
}
trap cleanup EXIT

chmod 700 "$TMP"
mkdir "$TMP/runtime" "$TMP/state" "$TMP/home" "$TMP/config" "$TMP/project"
chmod 700 "$TMP/runtime" "$TMP/state" "$TMP/home" "$TMP/config" "$TMP/project"
printf "config_version=5\n" >"$TMP/project/project.godot"

export XDG_RUNTIME_DIR="$TMP/runtime"
export XDG_STATE_HOME="$TMP/state"
export HOME="$TMP/home"

port=$(python3 - <<'PY'
import socket
s=socket.socket()
s.bind(("127.0.0.1",0))
print(s.getsockname()[1])
s.close()
PY
)
project=$(printf "a%.0s" $(seq 1 64))
secret=$(printf "b%.0s" $(seq 1 64))

cat >"$TMP/config/config.json" <<JSON
{
  "port": $port,
  "development_mode": true,
  "projects": [{
    "project": "$project",
    "root": "/workspace/godot-project",
    "secret": "$secret"
  }],
  "runner": null
}
JSON
chmod 600 "$TMP/config/config.json"

cp "$DRIVER" "$TMP/driver"
chmod 700 "$TMP/driver"
sha=$(sha256sum "$TMP/driver" | awk '{print $1}')
version=$(python3 - "$ROOT/Cargo.toml" <<'PY'
import sys, tomllib
with open(sys.argv[1], "rb") as handle:
    print(tomllib.load(handle)["workspace"]["package"]["version"])
PY
)

cat >"$TMP/driver.json" <<JSON
{
  "manifest_version": 1,
  "protocol": 2,
  "id": "godot",
  "version": "$version",
  "publisher": "semwright",
  "executable": "$TMP/driver",
  "sha256": "$sha",
  "application": {
    "desktop_id": "org.godotengine.Godot",
    "process_names": ["godot", "godot4"],
    "supported_versions": ["4.7.2"]
  },
  "transport": "stdio_v1",
  "mounts": [
    {"root": "godot-config", "read_only": true},
    {"root": "godot-project", "read_only": false}
  ],
  "system_config": [],
  "network": true,
  "resources": {
    "open_files": 128,
    "processes": 32,
    "cpu_seconds": 60,
    "address_space_bytes": 536870912,
    "file_size_bytes": 16777216
  },
  "request_timeout_ms": 5000,
  "interfaces": {
    "dynamic_capabilities": false,
    "cooperative_cancellation": true,
    "events": true,
    "progress": true,
    "artifacts": true,
    "health": true
  }
}
JSON
chmod 600 "$TMP/driver.json"

cat >"$TMP/daemon.toml" <<TOML
drivers = ["$TMP/driver.json"]
driver_network = true

[policy]
profile = "workspace"
allow = ["driver:godot"]

[[policy.filesystem]]
name = "godot-config"
path = "$TMP/config"
read = true
write = false

[[policy.filesystem]]
name = "godot-project"
path = "$TMP/project"
read = true
write = true
TOML
chmod 600 "$TMP/daemon.toml"

SOCKET="$TMP/runtime/broker.sock"
SESSION="$TMP/runtime/cli.session"
LOG="$TMP/daemon.log"
"$DAEMON" --config "$TMP/daemon.toml" --socket "$SOCKET" >"$LOG" 2>&1 &
DAEMON_PID=$!

for _ in $(seq 1 200); do
  [[ -S "$SOCKET" ]] && break
  if ! kill -0 "$DAEMON_PID" 2>/dev/null; then
    cat "$LOG" >&2
    exit 1
  fi
  sleep 0.05
done
[[ -S "$SOCKET" ]] || { cat "$LOG" >&2; exit 1; }

"$FAKE" --port "$port" --project "$project" --secret "$secret" >"$TMP/fake.out" 2>"$TMP/fake.err" &
FAKE_PID=$!

run() {
  "$CTL" --socket "$SOCKET" --session-file "$SESSION" --json "$@"
}

session_json=""
for _ in $(seq 1 120); do
  session_json=$(run execute driver.godot.session.list 2>/dev/null || true)
  if python3 - "$session_json" <<'PY' 2>/dev/null
import json,sys
value=json.loads(sys.argv[1])
raise SystemExit(0 if value["data"]["sessions"] else 1)
PY
  then
    break
  fi
  if ! kill -0 "$FAKE_PID" 2>/dev/null; then
    cat "$TMP/fake.err" >&2
    exit 1
  fi
  sleep 0.05
done

search=$(run capabilities search "" --provider driver:godot --limit 100)
doctor=$(run execute driver.godot.doctor)
sessions=$(run execute driver.godot.session.list)
sid=$(python3 -c 'import json,sys; print(json.loads(sys.argv[1])["data"]["sessions"][0]["session"])' "$sessions")
inspect=$(run execute driver.godot.project.inspect --args-json "$(printf '{"session":"%s"}' "$sid")")

python3 - "$search" "$doctor" "$sessions" "$inspect" <<'PY'
import json,sys
search,doctor,sessions,inspect=map(json.loads,sys.argv[1:])
rows=search["data"]["capabilities"]
assert len(rows)==53, len(rows)
assert all(row["provenance"]["provider"]=="driver:godot" for row in rows)
assert doctor["ok"] is True
assert doctor["execution"]["provenance"]["provider"]=="driver:godot"
assert len(sessions["data"]["sessions"])==1
assert inspect["ok"] is True
assert inspect["data"]["data"]["name"]=="Broker fixture"
assert inspect["data"]["data"]["engine"]=="4.7.2-stable (broker-fixture)"
assert inspect["execution"]["provenance"]["provider"]=="driver:godot"
print(json.dumps({
  "godot_driver_broker_smoke":"PASS",
  "capabilities":len(rows),
  "provider":"driver:godot",
  "authenticated_editor":True,
  "project_inspect":True,
},sort_keys=True))
PY

echo "Godot driver broker E2E: PASS"
