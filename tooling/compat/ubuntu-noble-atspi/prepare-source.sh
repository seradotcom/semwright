#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../../.." && pwd)
OUT=${1:?usage: prepare-source.sh OUT_DIR}
BASE_SHA=f55067272e0eaa54b4208fa46541ba7d435a3ac2
PATCH=semwright-backport-d442ee18-spicache-weak-ref.patch
SOURCE_URL=https://git.launchpad.net/ubuntu/+source/at-spi2-core

rm -rf "$OUT"
git clone --depth=1 --branch ubuntu/noble "$SOURCE_URL" "$OUT"
test "$(git -C "$OUT" rev-parse HEAD)" = "$BASE_SHA"

install -m 0644 "$ROOT/tooling/compat/ubuntu-noble-atspi/$PATCH"   "$OUT/debian/patches/$PATCH"
grep -qxF "$PATCH" "$OUT/debian/patches/series" || echo "$PATCH" >> "$OUT/debian/patches/series"

python3 - "$OUT/debian/changelog" <<'PY'
from pathlib import Path
import sys
p=Path(sys.argv[1])
old=p.read_text()
entry="""at-spi2-core (2.52.0-1build1+semwright1) noble; urgency=medium

  * Local compatibility backport for Ubuntu Noble:
    - Backport upstream commit d442ee182ec8fa095c6bc5298a17663cfc70cf9a.
    - Make SpiCache own its weak refs, avoiding dangling accessible pointers
      implicated in GNOME Shell SIGSEGV during heavy AT-SPI automation.
    - Temporary package until Ubuntu ships an equivalent upstream fix.

 -- Semwright compatibility build <local@semwright.invalid>  Fri, 25 Sep 2026 17:40:00 -0600

"""
if not old.startswith("at-spi2-core (2.52.0-1build1)"):
    raise SystemExit("unexpected Ubuntu Noble at-spi2-core baseline")
p.write_text(entry+old)
PY

git -C "$OUT" diff --check
head -1 "$OUT/debian/changelog" | grep -F "2.52.0-1build1+semwright1"
echo "prepared=$OUT"
