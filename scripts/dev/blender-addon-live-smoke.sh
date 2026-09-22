#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
BIN_DIR=${BIN_DIR:-"$ROOT/target/debug"}
BLENDER_BIN=${BLENDER_BIN:-/usr/local/bin/blender}
DAEMON="$BIN_DIR/semwrightd"
CTL="$BIN_DIR/computerctl"

for file in "$DAEMON" "$CTL" "$BLENDER_BIN"; do
  test -x "$file" || { echo "missing executable: $file" >&2; exit 2; }
done
command -v xvfb-run >/dev/null || { echo "xvfb-run is required" >&2; exit 5; }

TMP=$(mktemp -d)
DAEMON_PID=""
BLENDER_PID=""
cleanup() {
  if [[ -n "$DAEMON_PID" ]]; then
    kill -TERM "$DAEMON_PID" 2>/dev/null || true
    wait "$DAEMON_PID" 2>/dev/null || true
  fi
  if [[ -n "$BLENDER_PID" ]]; then
    kill -TERM -- "-$BLENDER_PID" 2>/dev/null || kill -TERM "$BLENDER_PID" 2>/dev/null || true
    for _ in $(seq 1 80); do
      kill -0 "$BLENDER_PID" 2>/dev/null || break
      sleep 0.05
    done
    kill -KILL -- "-$BLENDER_PID" 2>/dev/null || kill -KILL "$BLENDER_PID" 2>/dev/null || true
    wait "$BLENDER_PID" 2>/dev/null || true
  fi
  rm -rf "$TMP"
}
trap cleanup EXIT

chmod 700 "$TMP"
mkdir "$TMP/runtime" "$TMP/state" "$TMP/home" "$TMP/workspace"
chmod 700 "$TMP/runtime" "$TMP/state" "$TMP/home" "$TMP/workspace"
export XDG_RUNTIME_DIR="$TMP/runtime"
export XDG_STATE_HOME="$TMP/state"
export HOME="$TMP/home"
export SEMWRIGHT_BLENDER_WORKSPACE="$TMP/workspace"
export SEMWRIGHT_BLENDER_ADDON_ROOT="$ROOT/adapters/blender"

cat > "$TMP/bootstrap.py" <<'PY'
import os
import sys
import time
import bpy

sys.path.insert(0, os.environ["SEMWRIGHT_BLENDER_ADDON_ROOT"])
import semwright_blender

semwright_blender.register()
print("SEMWRIGHT_BLENDER_ADDON_READY", flush=True)
started = time.monotonic()

def watchdog():
    if time.monotonic() - started > 120:
        try:
            semwright_blender.unregister()
        finally:
            bpy.ops.wm.quit_blender()
        return None
    return 1.0

bpy.app.timers.register(watchdog, first_interval=1.0)
PY

BLENDER_LOG="$TMP/blender.log"
setsid xvfb-run -a -s "-screen 0 1280x720x24" \
  "$BLENDER_BIN" --factory-startup --disable-autoexec --no-splash \
  --python "$TMP/bootstrap.py" >"$BLENDER_LOG" 2>&1 &
BLENDER_PID=$!

BRIDGE="$TMP/runtime/semwright-blender/bridge.sock"
for _ in $(seq 1 400); do
  [[ -S "$BRIDGE" ]] && break
  if ! kill -0 "$BLENDER_PID" 2>/dev/null; then
    cat "$BLENDER_LOG" >&2
    exit 1
  fi
  sleep 0.05
done
[[ -S "$BRIDGE" ]] || { cat "$BLENDER_LOG" >&2; exit 1; }

python3 - "$BRIDGE" <<'PY'
import os, stat, sys
path = sys.argv[1]
st = os.stat(path)
assert stat.S_ISSOCK(st.st_mode)
assert stat.S_IMODE(st.st_mode) == 0o600
assert st.st_uid == os.getuid()
PY

cat > "$TMP/daemon.toml" <<TOML
blender_socket = "$BRIDGE"

[policy]
profile = "workspace"
allow = ["blender.observe", "blender.modify"]
TOML
chmod 600 "$TMP/daemon.toml"

SOCKET="$TMP/runtime/broker.sock"
SESSION="$TMP/runtime/cli.session"
DAEMON_LOG="$TMP/daemon.log"
"$DAEMON" --config "$TMP/daemon.toml" --socket "$SOCKET" >"$DAEMON_LOG" 2>&1 &
DAEMON_PID=$!
for _ in $(seq 1 200); do
  [[ -S "$SOCKET" ]] && break
  if ! kill -0 "$DAEMON_PID" 2>/dev/null; then
    cat "$DAEMON_LOG" >&2
    exit 1
  fi
  sleep 0.05
done
[[ -S "$SOCKET" ]] || { cat "$DAEMON_LOG" >&2; exit 1; }

run() {
  "$CTL" --socket "$SOCKET" --session-file "$SESSION" --json "$@"
}

search=$(run capabilities search blender --provider blender-native --limit 100)
status=$(run execute blender.status)
scene=$(run execute blender.scene.inspect)
create=$(run execute blender.object.create --args-json '{"name":"InteractiveCube","primitive":"cube","location":[1,2,3]}')
get=$(run execute blender.object.get --args-json '{"name":"InteractiveCube"}')
material=$(run execute blender.material.create --args-json '{"name":"InteractiveMaterial","color":[0.3,0.5,0.9,1],"roughness":0.4,"metallic":0.05}')
assign=$(run execute blender.material.assign --args-json '{"object":"InteractiveCube","material":"InteractiveMaterial"}')
settings=$(run execute blender.render.settings --args-json '{"width":64,"height":64,"engine":"CYCLES","samples":1}')

python3 - "$search" "$status" "$scene" "$create" "$get" "$material" "$assign" "$settings" <<'PY'
import json, sys
search, status, scene, create, get, material, assign, settings = map(json.loads, sys.argv[1:])
rows = search["data"]["capabilities"]
ids = {row["id"] for row in rows}
for required in [
    "blender.status", "blender.scene.inspect", "blender.object.create",
    "blender.object.get", "blender.material.create", "blender.material.assign",
    "blender.render.settings",
]:
    assert required in ids, required
assert all(row["provenance"]["provider"] == "blender-native" for row in rows)
assert status["data"]["connected"] is True
assert status["data"]["arbitrary_python"] is False
assert status["execution"]["provenance"]["provider"] == "blender-native"
assert status["data"]["version"][:2] == [4, 5]
assert scene["ok"] is True
assert create["data"]["object"]["name"] == "InteractiveCube"
assert get["data"]["object"]["name"] == "InteractiveCube"
assert material["data"]["name"] == "InteractiveMaterial"
assert assign["data"]["changed"] is True
assert settings["data"]["engine"] == "CYCLES"
print(json.dumps({
    "blender_addon_smoke": "PASS",
    "capabilities": len(rows),
    "provider": status["execution"]["provenance"]["provider"],
    "version": status["data"]["version"],
    "object_create": True,
    "material_assign": True,
    "typed_render_setting": True,
}, sort_keys=True))
PY

grep -q "SEMWRIGHT_BLENDER_ADDON_READY" "$BLENDER_LOG"
echo "Blender interactive add-on broker E2E: PASS"
