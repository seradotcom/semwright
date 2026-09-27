#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
BIN_DIR=${BIN_DIR:-"$ROOT/target/debug"}
DAEMON="$BIN_DIR/semwrightd"
CTL="$BIN_DIR/semwright"
DRIVER="$BIN_DIR/semwright-blender-driver"
SANDBOX="$BIN_DIR/semwright-sandbox"

for file in "$DAEMON" "$CTL" "$DRIVER" "$SANDBOX"; do
  test -x "$file" || { echo "missing executable: $file" >&2; exit 2; }
done
command -v bwrap >/dev/null
if [[ ! -x /usr/local/bin/blender && ! -x /usr/bin/blender ]]; then
  echo "Blender binary is missing" >&2
  exit 5
fi
test -d /etc/fonts

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
mkdir "$TMP/runtime" "$TMP/state" "$TMP/home" "$TMP/workspace"
chmod 700 "$TMP/runtime" "$TMP/state" "$TMP/home" "$TMP/workspace"
export XDG_RUNTIME_DIR="$TMP/runtime"
export XDG_STATE_HOME="$TMP/state"
export HOME="$TMP/home"

cp "$DRIVER" "$TMP/driver"
chmod 700 "$TMP/driver"
sha=$(sha256sum "$TMP/driver" | awk '{print $1}')
version=$(python3 - "$ROOT/Cargo.toml" <<'PY'
import sys, tomllib
with open(sys.argv[1], "rb") as handle:
    print(tomllib.load(handle)["workspace"]["package"]["version"])
PY
)

cat > "$TMP/driver.json" <<JSON
{
  "manifest_version": 1,
  "protocol": 1,
  "id": "blender",
  "version": "$version",
  "publisher": "semwright",
  "executable": "$TMP/driver",
  "sha256": "$sha",
  "application": {
    "desktop_id": "org.blender.Blender",
    "process_names": ["blender"],
    "supported_versions": ["4.5.14"]
  },
  "transport": "stdio_v1",
  "mounts": [
    {"root": "workspace", "read_only": false}
  ],
  "system_config": [
    {"root": "font-config", "destination": "/etc/fonts"}
  ],
  "network": false,
  "resources": {
    "open_files": 256,
    "processes": 64,
    "cpu_seconds": 300,
    "address_space_bytes": 4294967296,
    "file_size_bytes": 1073741824
  },
  "request_timeout_ms": 120000,
  "interfaces": {
    "dynamic_capabilities": false,
    "cooperative_cancellation": false,
    "events": false,
    "health": true
  }
}
JSON
chmod 600 "$TMP/driver.json"

cat > "$TMP/daemon.toml" <<TOML
drivers = ["$TMP/driver.json"]
driver_network = false

[policy]
profile = "workspace"
allow = ["driver:blender"]

[[policy.filesystem]]
name = "workspace"
path = "$TMP/workspace"
read = true
write = true

[[policy.filesystem]]
name = "font-config"
path = "/etc/fonts"
read = true
write = false
TOML
chmod 600 "$TMP/daemon.toml"

SOCKET="$TMP/runtime/broker.sock"
SESSION="$TMP/runtime/cli.session"
LOG="$TMP/daemon.log"
"$DAEMON" --config "$TMP/daemon.toml" --socket "$SOCKET" >"$LOG" 2>&1 &
DAEMON_PID=$!
for _ in $(seq 1 300); do
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

search=$(run capabilities search "" --provider driver:blender --limit 100)
status=$(run execute driver.blender.status)
summary=$(run execute driver.blender.introspect.summary)
operators=$(run execute driver.blender.introspect.operators --args-json '{"query":"primitive_cube_add","limit":32}')
types=$(run execute driver.blender.introspect.types --args-json '{"query":"Mesh","limit":32}')
scene=$(run execute driver.blender.scene.inspect)
create=$(run execute driver.blender.object.create --args-json '{"name":"BrokerCube","primitive":"cube","location":[1,2,3]}')
material=$(run execute driver.blender.material.create --args-json '{"name":"BrokerMaterial","color":[0.3,0.5,0.9,1],"roughness":0.4,"metallic":0.05}')
assign=$(run execute driver.blender.material.assign --args-json '{"object":"BrokerCube","material":"BrokerMaterial"}')
settings=$(run execute driver.blender.render.settings --args-json '{"width":64,"height":64,"engine":"CYCLES","samples":1}')
render=$(run execute driver.blender.render --args-json '{"path":"broker-preview.png"}')

python3 - "$search" "$status" "$summary" "$operators" "$types" "$scene" "$create" "$material" "$assign" "$settings" "$render" <<'PY'
import json, sys
(
    search, status, summary, operators, types, scene, create,
    material, assign, settings, render,
) = map(json.loads, sys.argv[1:])
rows = search["data"]["capabilities"]
ids = {row["id"] for row in rows}
expected = {
    "driver.blender.status",
    "driver.blender.object.create",
    "driver.blender.render",
    "driver.blender.introspect.summary",
    "driver.blender.introspect.operators",
    "driver.blender.introspect.types",
}
assert expected <= ids
assert all(
    row["provenance"]["provider"] == "driver:blender"
    for row in rows if row["id"] in expected
)
assert status["data"]["connected"] is True
assert status["data"]["arbitrary_python"] is False
assert status["execution"]["provenance"]["provider"] == "driver:blender"
assert summary["data"]["version"][:2] == [4, 5]
assert summary["data"]["rna_types"] > 100
assert summary["data"]["operators"] > 100
assert summary["data"]["generic_operator_invoke"] is False
assert any("primitive_cube_add" in row["id"] for row in operators["data"]["items"])
assert any(row["identifier"] == "Mesh" for row in types["data"]["items"])
assert scene["ok"] is True
assert create["data"]["object"]["name"] == "BrokerCube"
assert material["data"]["name"] == "BrokerMaterial"
assert assign["data"]["changed"] is True
assert settings["data"]["engine"] == "CYCLES"
assert render["data"]["path"] == "broker-preview.png"
print(json.dumps({
    "blender_driver_smoke": "PASS",
    "capabilities": len(rows),
    "provider": status["execution"]["provenance"]["provider"],
    "rna_types": summary["data"]["rna_types"],
    "operators": summary["data"]["operators"],
    "object_create": True,
    "material_assign": True,
    "render": True,
}, sort_keys=True))
PY

python3 - "$TMP/workspace/broker-preview.png" <<'PY'
import pathlib, sys
data = pathlib.Path(sys.argv[1]).read_bytes()
assert data.startswith(b"\x89PNG\r\n\x1a\n")
PY

echo "Blender driver broker E2E: PASS"
