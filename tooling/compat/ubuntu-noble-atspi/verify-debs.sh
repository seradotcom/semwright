#!/usr/bin/env bash
set -euo pipefail

DIR=${1:?usage: verify-debs.sh DEB_DIR}
EXPECTED=2.52.0-1build1+semwright1
BRIDGE=$(find "$DIR" -maxdepth 1 -type f -name 'libatk-bridge2.0-0t64_*_amd64.deb' | head -1)
test -n "$BRIDGE"

test "$(dpkg-deb -f "$BRIDGE" Package)" = "libatk-bridge2.0-0t64"
test "$(dpkg-deb -f "$BRIDGE" Version)" = "$EXPECTED"

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
dpkg-deb -x "$BRIDGE" "$TMP"
test -f "$TMP/usr/lib/x86_64-linux-gnu/libatk-bridge-2.0.so.0.0.0"
test -f "$TMP/usr/share/doc/libatk-bridge2.0-0t64/changelog.Debian.gz"
zgrep -F "d442ee182ec8fa095c6bc5298a17663cfc70cf9a"   "$TMP/usr/share/doc/libatk-bridge2.0-0t64/changelog.Debian.gz"

sha256sum "$DIR"/*.deb | sort > "$DIR/SHA256SUMS"
sha256sum -c "$DIR/SHA256SUMS"

python3 - "$DIR" "$BRIDGE" <<'PY'
from pathlib import Path
import hashlib, json, sys
root=Path(sys.argv[1]); bridge=Path(sys.argv[2])
def sha(p):
    h=hashlib.sha256(); h.update(p.read_bytes()); return h.hexdigest()
manifest={
  "status":"PASS",
  "ubuntu_base":"2.52.0-1build1",
  "local_version":"2.52.0-1build1+semwright1",
  "ubuntu_source_sha":"f55067272e0eaa54b4208fa46541ba7d435a3ac2",
  "upstream_fix":"d442ee182ec8fa095c6bc5298a17663cfc70cf9a",
  "bridge_package":bridge.name,
  "bridge_sha256":sha(bridge),
  "packages":{p.name:sha(p) for p in sorted(root.glob("*.deb"))},
}
(root/"BACKPORT_MANIFEST.json").write_text(json.dumps(manifest,indent=2)+"\n")
PY

cat "$DIR/BACKPORT_MANIFEST.json"
