#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd -P)
BIN_DIR=${BIN_DIR:-"$ROOT/target/debug"}
DAEMON="$BIN_DIR/semwrightd"
CTL="$BIN_DIR/semwright"
DRIVER="$BIN_DIR/semwright-blender-driver"
SESSION_RUNNER="$BIN_DIR/semwright-blender-session-runner"
SANDBOX="$BIN_DIR/semwright-sandbox"

for file in "$DAEMON" "$CTL" "$DRIVER" "$SESSION_RUNNER" "$SANDBOX"; do
  test -x "$file" || { echo "missing executable: $file" >&2; exit 2; }
done
DAEMON=$(realpath -e "$DAEMON")
CTL=$(realpath -e "$CTL")
DRIVER=$(realpath -e "$DRIVER")
SESSION_RUNNER=$(realpath -e "$SESSION_RUNNER")
SANDBOX=$(realpath -e "$SANDBOX")
command -v bwrap >/dev/null
BLENDER_ROOT=${SEMWRIGHT_TEST_BLENDER_ROOT:-}
if [[ -z "$BLENDER_ROOT" ]]; then
  blender_on_path=$(command -v blender || true)
  if [[ -z "$blender_on_path" ]]; then
    echo "Blender binary is missing; set SEMWRIGHT_TEST_BLENDER_ROOT" >&2
    exit 5
  fi
  blender_real=$(readlink -f "$blender_on_path")
  BLENDER_ROOT=$(dirname "$blender_real")
fi
BLENDER_ROOT=$(realpath -e "$BLENDER_ROOT")
BLENDER_BIN=$(realpath -e "$BLENDER_ROOT/blender")
case "$BLENDER_BIN" in
  "$BLENDER_ROOT"/*) ;;
  *) echo "Blender executable escaped its canonical runtime root" >&2; exit 5 ;;
esac
test -x "$BLENDER_BIN" || { echo "Blender runtime is missing $BLENDER_BIN" >&2; exit 5; }
test "$("$BLENDER_BIN" --version | head -1)" = "Blender 4.5.14 LTS"
for required in lib 4.5/scripts 4.5/extensions 4.5/datafiles 4.5/python; do
  required_path=$(realpath -e "$BLENDER_ROOT/$required")
  case "$required_path" in
    "$BLENDER_ROOT"/*) ;;
    *) echo "Blender runtime component escaped root: $required" >&2; exit 5 ;;
  esac
  test -d "$required_path" || { echo "Blender runtime missing $required" >&2; exit 5; }
done
BLENDER_SHA=$(sha256sum "$BLENDER_BIN" | awk '{print $1}')
SESSION_RUNNER_SHA=$(sha256sum "$SESSION_RUNNER" | awk '{print $1}')
FONT_CONFIG_ROOT=$(realpath -e /etc/fonts)
test -d "$FONT_CONFIG_ROOT"

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
mkdir "$TMP/runtime" "$TMP/state" "$TMP/home" "$TMP/workspace" "$TMP/scratch"
chmod 700 "$TMP/runtime" "$TMP/state" "$TMP/home" "$TMP/workspace" "$TMP/scratch"
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
  "protocol": 8,
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
    {"root": "workspace", "read_only": false},
    {"root": "blender-runtime", "read_only": true},
    {"root": "scratch", "read_only": false},
    {"root": "font-config", "read_only": true}
  ],
  "system_config": [],
  "tools": [
    {
      "root": "blender-session-runner",
      "name": "blender-session-runner",
      "sha256": "$SESSION_RUNNER_SHA",
      "mounts": ["workspace", "blender-runtime", "scratch", "font-config"],
      "dependencies": ["blender"]
    },
    {
      "root": "blender-executable",
      "name": "blender",
      "sha256": "$BLENDER_SHA",
      "mounts": [],
      "dependencies": []
    }
  ],
  "network": false,
  "resources": {
    "open_files": 256,
    "processes": 64,
    "cpu_seconds": 300,
    "operation_cpu_seconds": 0,
    "address_space_bytes": 4294967296,
    "file_size_bytes": 1073741824
  },
  "request_timeout_ms": 300000,
  "interfaces": {
    "dynamic_capabilities": false,
    "cooperative_cancellation": true,
    "events": false,
    "health": true,
    "progress": false,
    "artifacts": false,
    "native_refs": false,
    "host_tools": true
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
path = "$FONT_CONFIG_ROOT"
read = true
write = false

[[policy.filesystem]]
name = "blender-runtime"
path = "$BLENDER_ROOT"
read = true
write = false

[[policy.filesystem]]
name = "scratch"
path = "$TMP/scratch"
read = true
write = true

[[policy.filesystem]]
name = "blender-session-runner"
path = "$SESSION_RUNNER"
read = true
write = false

[[policy.filesystem]]
name = "blender-executable"
path = "$BLENDER_BIN"
read = true
write = false
TOML
chmod 600 "$TMP/daemon.toml"

SOCKET="$TMP/runtime/broker.sock"
SESSION="$TMP/runtime/cli.session"
LOG="$TMP/daemon.log"
"$DAEMON" --config "$TMP/daemon.toml" --socket "$SOCKET" >"$LOG" 2>&1 &
DAEMON_PID=$!

daemon_startup_diagnostics() {
  echo "--- semwrightd startup diagnostics ---" >&2
  ps -o pid=,ppid=,stat=,etime=,cmd= -p "$DAEMON_PID" >&2 2>/dev/null || true
  ps -o pid=,ppid=,stat=,etime=,cmd= --ppid "$DAEMON_PID" >&2 2>/dev/null || true
  if [[ -r "/proc/$DAEMON_PID/wchan" ]]; then
    printf 'wchan=' >&2
    head -c 128 "/proc/$DAEMON_PID/wchan" >&2 || true
    printf '\n' >&2
  fi
  if [[ -f "$LOG" ]]; then
    echo "--- semwrightd log tail ---" >&2
    tail -c 16384 "$LOG" >&2 || true
    printf '\n' >&2
  fi
}

ready=0
for _ in $(seq 1 1200); do
  if [[ -S "$SOCKET" ]]; then
    ready=1
    break
  fi
  if ! kill -0 "$DAEMON_PID" 2>/dev/null; then
    daemon_startup_diagnostics
    exit 1
  fi
  sleep 0.05
done
if [[ "$ready" != "1" ]]; then
  echo "semwrightd did not publish its broker socket within 60 seconds" >&2
  daemon_startup_diagnostics
  exit 1
fi

run() {
  "$CTL" --socket "$SOCKET" --session-file "$SESSION" --json "$@"
}

search_pages="$TMP/blender-capabilities.jsonl"
: > "$search_pages"
offset=0
while true; do
  page_file="$TMP/blender-capabilities-page-$offset.json"
  run capabilities search "" --provider driver:blender --limit 100 --offset "$offset" > "$page_file"
  cat "$page_file" >> "$search_pages"
  printf '\n' >> "$search_pages"
  next_offset=$(python3 - "$page_file" <<'PY_PAGE'
import json, pathlib, sys
value = json.loads(pathlib.Path(sys.argv[1]).read_text())["data"]["next_offset"]
print("" if value is None else value)
PY_PAGE
  )
  [[ -n "$next_offset" ]] || break
  offset=$next_offset
done
search_file="$TMP/blender-capabilities.json"
python3 - "$search_pages" > "$search_file" <<'PY_SEARCH'
import json, pathlib, sys
pages = [json.loads(line) for line in pathlib.Path(sys.argv[1]).read_text().splitlines() if line]
assert pages
revisions = {page["data"]["revision"] for page in pages}
assert len(revisions) == 1, "catalog revision changed during paginated discovery"
merged = pages[0]
merged["data"]["capabilities"] = [
    capability
    for page in pages
    for capability in page["data"]["capabilities"]
]
merged["data"]["offset"] = 0
merged["data"]["next_offset"] = None
print(json.dumps(merged, separators=(",", ":")))
PY_SEARCH
status_file="$TMP/blender-status.json"
summary_file="$TMP/blender-summary.json"
operators_file="$TMP/blender-operators.json"
types_file="$TMP/blender-types.json"
scene_file="$TMP/blender-scene.json"
create_file="$TMP/blender-create.json"
material_file="$TMP/blender-material.json"
assign_file="$TMP/blender-assign.json"
export_file="$TMP/blender-export-glb.json"
settings_file="$TMP/blender-settings.json"
render_file="$TMP/blender-render.json"

run execute driver.blender.status > "$status_file"
run execute driver.blender.introspect.summary > "$summary_file"
run execute driver.blender.introspect.operators --args-json '{"query":"primitive_cube_add","limit":32}' > "$operators_file"
run execute driver.blender.introspect.types --args-json '{"query":"Mesh","limit":32}' > "$types_file"
run execute driver.blender.scene.inspect > "$scene_file"
run execute driver.blender.object.create --args-json '{"name":"BrokerCube","primitive":"cube","location":[1,2,3]}' > "$create_file"
run execute driver.blender.collection.create --args-json '{"name":"BrokerExport"}' >/dev/null
run execute driver.blender.collection.link --args-json '{"object":"BrokerCube","collection":"BrokerExport"}' >/dev/null
run execute driver.blender.material.create --args-json '{"name":"BrokerMaterial","color":[0.3,0.5,0.9,1],"roughness":0.4,"metallic":0.05}' > "$material_file"
run execute driver.blender.material.assign --args-json '{"object":"BrokerCube","material":"BrokerMaterial"}' > "$assign_file"
run execute driver.blender.export.glb --args-json '{"collection":"BrokerExport","path":"broker-model.glb","animations":false}' > "$export_file"
run execute driver.blender.render.settings --args-json '{"width":64,"height":64,"engine":"CYCLES","samples":1}' > "$settings_file"
run execute driver.blender.render --args-json '{"path":"broker-preview.png"}' > "$render_file"

python3 - "$search_file" "$status_file" "$summary_file" "$operators_file" "$types_file" "$scene_file" "$create_file" "$material_file" "$assign_file" "$export_file" "$settings_file" "$render_file" <<'PY'
import json, pathlib, sys
(
    search, status, summary, operators, types, scene, create,
    material, assign, exported, settings, render,
) = [json.loads(pathlib.Path(path).read_text()) for path in sys.argv[1:]]
rows = search["data"]["capabilities"]
ids = {row["id"] for row in rows}
expected = {
    "driver.blender.status",
    "driver.blender.object.create",
    "driver.blender.export.glb",
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
assert exported["data"]["changed"] is True
assert exported["data"]["path"] == "broker-model.glb"
assert exported["data"]["format"] == "glb"
assert len(exported["data"]["sha256"]) == 64
assert exported["data"]["bytes"] > 20
assert exported["data"]["objects"] == 1
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
    "glb_export": True,
    "render": True,
}, sort_keys=True))
PY

python3 - "$TMP/workspace/broker-preview.png" "$TMP/workspace/broker-model.glb" <<'PY'
import pathlib, struct, sys
png = pathlib.Path(sys.argv[1]).read_bytes()
assert png.startswith(b"\x89PNG\r\n\x1a\n")
glb = pathlib.Path(sys.argv[2]).read_bytes()
assert len(glb) > 20
magic, version, length = struct.unpack("<4sII", glb[:12])
assert magic == b"glTF"
assert version == 2
assert length == len(glb)
PY

echo "Blender driver broker E2E: PASS"
