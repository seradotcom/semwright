#!/usr/bin/env bash
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
BIN_DIR=${BIN_DIR:-"$ROOT/target/debug"}
CTL="$BIN_DIR/computerctl"
FIXTURE="$BIN_DIR/semwright-driver-fixture"
SANDBOX="$BIN_DIR/semwright-sandbox"
for file in "$CTL" "$FIXTURE" "$SANDBOX"; do
  test -x "$file" || { echo "missing executable: $file" >&2; exit 2; }
done
command -v bwrap >/dev/null || { echo "bubblewrap unavailable" >&2; exit 5; }

TMP=$(mktemp -d)
cleanup() { rm -rf "$TMP"; }
trap cleanup EXIT
chmod 700 "$TMP"
mkdir "$TMP/runtime"
chmod 700 "$TMP/runtime"
export XDG_RUNTIME_DIR="$TMP/runtime"
cp "$FIXTURE" "$TMP/driver"
chmod 700 "$TMP/driver"
sha=$(sha256sum "$TMP/driver" | awk '{print $1}')
version=$(python3 - "$ROOT/Cargo.toml" <<'PY'
import sys, tomllib
with open(sys.argv[1], "rb") as f:
    print(tomllib.load(f)["workspace"]["package"]["version"])
PY
)
cat > "$TMP/manifest.json" <<JSON
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
chmod 600 "$TMP/manifest.json"

validate=$("$CTL" --json driver validate "$TMP/manifest.json")
inspect=$("$CTL" --json driver inspect "$TMP/manifest.json")
conformance=$("$CTL" --json driver conformance "$TMP/manifest.json")
python3 - "$validate" "$inspect" "$conformance" <<'PY'
import json, sys
validate, inspect, conformance = map(json.loads, sys.argv[1:])
assert validate["valid"] is True and validate["executed"] is False
assert inspect["identity"]["id"] == "driver:fixture"
assert inspect["identity"]["namespace"] == "driver.fixture."
assert inspect["policy_grants_changed"] is False
assert conformance["provider"] == "driver:fixture"
assert conformance["namespace"] == "driver.fixture."
assert conformance["capabilities"] == 1
assert conformance["sandboxed"] is True
assert conformance["persistent_process"] is True
assert conformance["health"] is True
assert conformance["executed_read_only"] is True
assert conformance["shutdown"] is True
print(json.dumps({
    "driver_conformance": "PASS",
    "provider": conformance["provider"],
    "capabilities": conformance["capabilities"],
    "sandboxed": conformance["sandboxed"],
    "persistent_process": conformance["persistent_process"]
}, sort_keys=True))
PY

scaffold="$TMP/scaffold"
scaffold_result=$("$CTL" --json driver scaffold fixturegen "$scaffold" --sdk-path "$ROOT/crates/driver-sdk")
python3 - "$scaffold_result" <<'PY'
import json, sys
result = json.loads(sys.argv[1])
assert result["installed"] is False
assert result["created"]
print("driver scaffold: PASS")
PY
cargo check --quiet --manifest-path "$scaffold/Cargo.toml"
echo "generated driver compile: PASS"
