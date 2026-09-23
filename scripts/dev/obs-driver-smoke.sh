#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
BIN_DIR=${BIN_DIR:-"$ROOT/target/debug"}
DAEMON="$BIN_DIR/semwrightd"
CTL="$BIN_DIR/computerctl"
DRIVER="$BIN_DIR/semwright-obs-driver"
SANDBOX="$BIN_DIR/semwright-sandbox"
FAKE_SERVER="$ROOT/crates/driver-obs/fixtures/fake-obs/server.py"

for file in "$DAEMON" "$CTL" "$DRIVER" "$SANDBOX"; do
  test -x "$file" || { echo "missing executable: $file" >&2; exit 2; }
done
for command in bwrap python3; do
  command -v "$command" >/dev/null || { echo "missing dependency: $command" >&2; exit 5; }
done
test -f "$FAKE_SERVER"

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
mkdir "$TMP/runtime" "$TMP/state" "$TMP/home" "$TMP/config"
chmod 700 "$TMP/runtime" "$TMP/state" "$TMP/home" "$TMP/config"
export XDG_RUNTIME_DIR="$TMP/runtime"
export XDG_STATE_HOME="$TMP/state"
export HOME="$TMP/home"
python3 "$FAKE_SERVER" --mode normal >"$TMP/fake.out" 2>"$TMP/fake.err" &
FAKE_PID=$!
for _ in $(seq 1 100); do
  [[ -s "$TMP/fake.out" ]] && break
  if ! kill -0 "$FAKE_PID" 2>/dev/null; then
    cat "$TMP/fake.err" >&2
    exit 1
  fi
  sleep 0.05
done
[[ -s "$TMP/fake.out" ]] || { cat "$TMP/fake.err" >&2; exit 1; }
port=$(python3 -c 'import json,sys; print(json.loads(open(sys.argv[1]).readline())["port"])' "$TMP/fake.out")

cat >"$TMP/config/config.json" <<JSON
{
  "address": "127.0.0.1",
  "port": $port,
  "connect_timeout_ms": 2000,
  "request_timeout_ms": 3000,
  "reconnect_limit": 2,
  "event_capacity": 64,
  "secret_socket": false,
  "allow_stream_start": false
}
JSON
chmod 600 "$TMP/config/config.json"

cp "$DRIVER" "$TMP/driver"
chmod 700 "$TMP/driver"
sha=$(sha256sum "$TMP/driver" | awk '{print $1}')
version=$(python3 -c 'import tomllib,sys; print(tomllib.load(open(sys.argv[1],"rb"))["workspace"]["package"]["version"])' "$ROOT/Cargo.toml")
cat >"$TMP/driver.json" <<JSON
{
  "manifest_version": 1,
  "protocol": 1,
  "id": "obs",
  "version": "$version",
  "publisher": "semwright",
  "executable": "$TMP/driver",
  "sha256": "$sha",
  "application": {
    "desktop_id": "com.obsproject.Studio",
    "process_names": ["obs"],
    "supported_versions": []
  },
  "transport": "stdio_v1",
  "mounts": [],
  "system_config": [
    {"root": "obs-config", "destination": "/etc/semwright-obs"}
  ],
  "network": true,
  "resources": {
    "open_files": 128,
    "processes": 32,
    "cpu_seconds": 120,
    "address_space_bytes": 1073741824,
    "file_size_bytes": 16777216
  },
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

cat >"$TMP/daemon.toml" <<TOML
drivers = ["$TMP/driver.json"]
driver_network = true

[policy]
profile = "workspace"
allow = ["driver:obs"]

[[policy.filesystem]]
name = "obs-config"
path = "$TMP/config"
read = true
write = false
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

run() {
  "$CTL" --socket "$SOCKET" --session-file "$SESSION" --json "$@"
}

search=$(run capabilities search "" --provider driver:obs --limit 100)
doctor=$(run execute driver.obs.doctor)
version_out=$(run execute driver.obs.version)
scenes=$(run execute driver.obs.scene.list)
current=$(run execute driver.obs.scene.current.get)
inputs=$(run execute driver.obs.input.list)
record=$(run execute driver.obs.record.status)

read -r scene_ref generation input_ref < <(python3 -c '
import json,sys
scenes=json.loads(sys.argv[1])["data"]; inputs=json.loads(sys.argv[2])["data"]
print(scenes["data"]["scenes"][1]["ref"], scenes["generation"], inputs["data"]["inputs"][0]["ref"])
' "$scenes" "$inputs")

scene_set=$(run execute driver.obs.scene.current.set --args-json "$(printf '{"scene_ref":"%s","expected_generation":%s}' "$scene_ref" "$generation")")
# The graph mutation invalidates prior refs. Refresh before the next mutation.
inputs_after=$(run execute driver.obs.input.list)
read -r input_ref2 generation2 < <(python3 -c '
import json,sys
v=json.loads(sys.argv[1])["data"]; print(v["data"]["inputs"][0]["ref"], v["generation"])
' "$inputs_after")
mute_set=$(run execute driver.obs.input.mute.set --args-json "$(printf '{"input_ref":"%s","muted":true,"expected_muted":false,"expected_generation":%s}' "$input_ref2" "$generation2")")

set +e
stream_denied=$(run execute driver.obs.stream.start --args-json "$(printf '{"expected_generation":%s,"expected_active":false}' "$generation2")")
stream_rc=$?
set -e
[[ "$stream_rc" -ne 0 ]]

python3 - "$search" "$doctor" "$version_out" "$scenes" "$current" "$inputs" "$record" "$scene_set" "$mute_set" "$stream_denied" <<'PY'
import json, sys
search, doctor, version, scenes, current, inputs, record, scene_set, mute_set, stream_denied = map(json.loads, sys.argv[1:])
rows = search["data"]["capabilities"]
assert len(rows) == 65
assert all(row["provenance"]["provider"] == "driver:obs" for row in rows)
assert doctor["ok"] is True and doctor["data"]["data"]["connected"] is True
assert doctor["execution"]["provenance"]["provider"] == "driver:obs"
assert version["data"]["data"]["rpc_version"] == 1
assert len(scenes["data"]["data"]["scenes"]) == 3
assert current["data"]["data"]["name"] == "Main"
assert len(inputs["data"]["data"]["inputs"]) == 4
assert record["data"]["data"]["active"] is False
assert scene_set["data"]["data"]["accepted"] is True
assert mute_set["data"]["data"]["accepted"] is True
assert stream_denied["ok"] is False
assert stream_denied["error"]["code"] == "ConsentRequired"
assert stream_denied["execution"]["policy_decision"] == "require_confirmation"
print(json.dumps({
    "obs_driver_broker_smoke": "PASS",
    "capabilities": len(rows),
    "provider": doctor["execution"]["provenance"]["provider"],
    "scene_mutation": True,
    "input_mute_mutation": True,
    "stream_start_requires_trusted_confirmation": True,
}, sort_keys=True))
PY

echo "OBS driver broker E2E: PASS"
