#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
BIN_DIR=${BIN_DIR:-"$ROOT/target/debug"}
CTL="$BIN_DIR/computerctl"
test -x "$CTL" || { echo "missing computerctl: $CTL" >&2; exit 2; }
test -x /usr/bin/true || { echo "missing /usr/bin/true" >&2; exit 5; }

TMP=$(mktemp -d)
cleanup() { rm -rf "$TMP"; }
trap cleanup EXIT
chmod 700 "$TMP"
mkdir -m 700 "$TMP/repo" "$TMP/data" "$TMP/config"

digest=$(sha256sum /usr/bin/true | awk '{print $1}')
cat > "$TMP/manifest.json" <<JSON
{
  "manifest_version": 1,
  "protocol": 1,
  "id": "fixture",
  "version": "1.2.3",
  "publisher": "semwright-tests",
  "executable": "/usr/bin/true",
  "sha256": "$digest",
  "application": {
    "desktop_id": "org.example.Fixture",
    "process_names": ["true"],
    "supported_versions": ["1"]
  },
  "transport": "stdio_v1",
  "mounts": [],
  "system_config": [],
  "network": false,
  "resources": {
    "open_files": 128,
    "processes": 32,
    "cpu_seconds": 20,
    "address_space_bytes": 536870912,
    "file_size_bytes": 16777216
  },
  "request_timeout_ms": 30000,
  "interfaces": {
    "dynamic_capabilities": false,
    "cooperative_cancellation": false,
    "events": false,
    "health": true
  }
}
JSON

package="$TMP/repo/fixture-1.2.3.swdp"
created=$("$CTL" --json driver package create "$TMP/manifest.json" "$package")
package_sha=$(python3 -c 'import json,sys; print(json.load(sys.stdin)["package_sha256"])' <<<"$created")
package_bytes=$(stat -c %s "$package")
version=$("$CTL" --version | awk '{print $2}')
cat > "$TMP/repo/index.json" <<JSON
{
  "index_version": 1,
  "drivers": [{
    "id": "fixture",
    "version": "1.2.3",
    "publisher": "semwright-tests",
    "package": "fixture-1.2.3.swdp",
    "package_sha256": "$package_sha",
    "package_bytes": $package_bytes,
    "semwright": "=$version",
    "application_versions": ["1"]
  }]
}
JSON

"$CTL" --json driver index validate "$TMP/repo/index.json" >/dev/null
search=$("$CTL" --json driver index search "$TMP/repo/index.json" fixture --application-version 1)
python3 - "$search" <<'PY'
import json,sys
d=json.loads(sys.argv[1])
assert len(d["drivers"]) == 1
assert d["drivers"][0]["compatible"] is True
assert d["drivers"][0]["id"] == "fixture"
PY

dry=$("$CTL" --json --dry-run driver install "$TMP/repo/index.json" fixture \
  --application-version 1 --data-dir "$TMP/data" --config-dir "$TMP/config")
python3 - "$dry" <<'PY'
import json,sys
d=json.loads(sys.argv[1])
assert d["installed"] is False and d["dry_run"] is True
assert d["executed"] is False and d["policy_grants_changed"] is False
PY
test ! -e "$TMP/data/fixture/1.2.3"

installed=$("$CTL" --json driver install "$TMP/repo/index.json" fixture \
  --application-version 1 --data-dir "$TMP/data" --config-dir "$TMP/config")
python3 - "$installed" <<'PY'
import json,sys
d=json.loads(sys.argv[1])
assert d["installed"] is True
assert d["executed"] is False
assert d["policy_grants_changed"] is False
PY

cmp /usr/bin/true "$TMP/data/fixture/1.2.3/driver"
test "$(stat -c %a "$TMP/data/fixture/1.2.3/driver")" = 700
test "$(stat -c %a "$TMP/config/fixture.json")" = 600
grep -q "$TMP/data/fixture/1.2.3/driver" "$TMP/config/fixture.json"

"$CTL" --json driver remove fixture 1.2.3 \
  --data-dir "$TMP/data" --config-dir "$TMP/config" >/dev/null
test ! -e "$TMP/data/fixture/1.2.3"
test ! -e "$TMP/config/fixture.json"

echo "Driver registry/distribution smoke: PASS"
