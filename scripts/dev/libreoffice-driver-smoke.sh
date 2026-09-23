#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
BIN_DIR=${BIN_DIR:-"$ROOT/target/debug"}
DAEMON="$BIN_DIR/semwrightd"
CTL="$BIN_DIR/semwright"
DRIVER="$BIN_DIR/semwright-libreoffice-driver"
SANDBOX="$BIN_DIR/semwright-sandbox"

for file in "$DAEMON" "$CTL" "$DRIVER" "$SANDBOX"; do
  test -x "$file" || { echo "missing executable: $file" >&2; exit 2; }
done
for command in bwrap soffice python3 unzip; do
  command -v "$command" >/dev/null || { echo "missing dependency: $command" >&2; exit 5; }
done
python3 -c 'import uno' >/dev/null

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
mkdir "$TMP/runtime" "$TMP/state" "$TMP/home" "$TMP/documents"
chmod 700 "$TMP/runtime" "$TMP/state" "$TMP/home" "$TMP/documents"
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
  "id": "libreoffice",
  "version": "$version",
  "publisher": "semwright",
  "executable": "$TMP/driver",
  "sha256": "$sha",
  "application": {
    "desktop_id": "org.libreoffice.LibreOffice",
    "process_names": ["soffice.bin"],
    "supported_versions": []
  },
  "transport": "stdio_v1",
  "mounts": [
    {"root": "workspace", "read_only": false}
  ],
  "system_config": [
    {"root": "libreoffice-config", "destination": "/etc/libreoffice"},
    {"root": "font-config", "destination": "/etc/fonts"}
  ],
  "network": false,
  "resources": {
    "open_files": 128,
    "processes": 32,
    "cpu_seconds": 120,
    "address_space_bytes": 2147483648,
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
chmod 600 "$TMP/driver.json"

cat > "$TMP/daemon.toml" <<TOML
drivers = ["$TMP/driver.json"]
driver_network = false

[policy]
profile = "workspace"
allow = ["driver:libreoffice"]

[[policy.filesystem]]
name = "workspace"
path = "$TMP/documents"
read = true
write = true

[[policy.filesystem]]
name = "libreoffice-config"
path = "/etc/libreoffice"
read = true
write = false

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
for _ in $(seq 1 160); do
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

search=$(run capabilities search libreoffice --provider driver:libreoffice)
status=$(run execute driver.libreoffice.status)
writer_create=$(run execute driver.libreoffice.writer.create --args-json '{"path":"note.odt","text":"Semwright UNO integration"}')
writer_read=$(run execute driver.libreoffice.writer.read --args-json '{"path":"note.odt"}')
calc_create=$(run execute driver.libreoffice.calc.create --args-json '{"path":"sheet.ods","cells":{"A1":"hello","B1":0,"C1":42.5}}')
calc_zero=$(run execute driver.libreoffice.calc.get --args-json '{"path":"sheet.ods","cell":"B1"}')
calc_set=$(run execute driver.libreoffice.calc.set --args-json '{"path":"sheet.ods","cell":"A2","value":"updated"}')
calc_read=$(run execute driver.libreoffice.calc.get --args-json '{"path":"sheet.ods","cell":"A2"}')
pdf_export=$(run execute driver.libreoffice.export.pdf --args-json '{"path":"note.odt","output":"note.pdf"}')

set +e
duplicate=$(run execute driver.libreoffice.writer.create --args-json '{"path":"note.odt","text":"must not overwrite"}')
duplicate_rc=$?
set -e
[[ "$duplicate_rc" -ne 0 ]]

python3 - "$search" "$status" "$writer_create" "$writer_read" "$calc_create" "$calc_zero" "$calc_set" "$calc_read" "$pdf_export" "$duplicate" <<'PY'
import json, sys
(
    search, status, writer_create, writer_read, calc_create,
    calc_zero, calc_set, calc_read, pdf_export, duplicate
) = map(json.loads, sys.argv[1:])

rows = search["data"]["capabilities"]
ids = {row["id"] for row in rows}
expected = {
    "driver.libreoffice.status",
    "driver.libreoffice.writer.create",
    "driver.libreoffice.writer.read",
    "driver.libreoffice.calc.create",
    "driver.libreoffice.calc.get",
    "driver.libreoffice.calc.set",
    "driver.libreoffice.export.pdf",
}
assert expected <= ids
assert all(
    row["provenance"]["provider"] == "driver:libreoffice"
    for row in rows if row["id"] in expected
)
assert status["ok"] is True
assert status["data"]["connected"] is True
assert status["data"]["product"] == "LibreOffice"
assert status["execution"]["provenance"]["provider"] == "driver:libreoffice"
assert writer_create["ok"] is True and writer_create["data"]["created"] is True
assert writer_read["data"]["text"] == "Semwright UNO integration"
assert calc_create["data"]["cells_written"] == 3
assert calc_zero["data"]["kind"] == "number"
assert calc_zero["data"]["value"] == 0
assert calc_set["data"]["updated"] is True
assert calc_read["data"]["kind"] == "text"
assert calc_read["data"]["value"] == "updated"
assert pdf_export["data"]["exported"] is True
assert duplicate["ok"] is False
assert duplicate["error"]["code"] == "Conflict"

print(json.dumps({
    "libreoffice_driver_smoke": "PASS",
    "capabilities": len(expected),
    "provider": status["execution"]["provenance"]["provider"],
    "writer_roundtrip": True,
    "calc_zero_is_number": True,
    "calc_mutation": True,
    "pdf_export": True,
    "no_overwrite": True,
}, sort_keys=True))
PY

test -s "$TMP/documents/note.odt"
test -s "$TMP/documents/sheet.ods"
test -s "$TMP/documents/note.pdf"
unzip -p "$TMP/documents/note.odt" content.xml | grep -q "Semwright UNO integration"
unzip -p "$TMP/documents/sheet.ods" content.xml | grep -q "updated"
head -c 5 "$TMP/documents/note.pdf" | grep -q '%PDF-'

echo "LibreOffice driver broker E2E: PASS"
