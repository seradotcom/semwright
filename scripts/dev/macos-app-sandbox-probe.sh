#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "macOS App Sandbox probe requires Darwin" >&2
  exit 2
fi

ROOT="${RUNNER_TEMP:-${TMPDIR:-/tmp}}/semwright-app-sandbox-probe-$$"
DENIED_ROOT="$HOME/semwright-app-sandbox-denied-$$"
RO="$ROOT/ro"
RW="$ROOT/rw"
BUILD="$ROOT/build"
mkdir -p "$RO" "$RW" "$BUILD" "$DENIED_ROOT"
cleanup() {
  kill "${SERVER_PID:-}" 2>/dev/null || true
  chmod -R u+w "$ROOT" "$DENIED_ROOT" 2>/dev/null || true
  rm -rf "$ROOT" "$DENIED_ROOT"
}
trap cleanup EXIT

printf 'allowed-ro' > "$RO/input.txt"
printf 'denied' > "$DENIED_ROOT/secret.txt"
chmod 0555 "$RO"
chmod 0700 "$RW" "$DENIED_ROOT"

cc -std=c17 -Wall -Wextra -Werror   crates/platform-macos-sys/tests/fixtures/app_sandbox_parent.c   -o "$BUILD/parent"
cc -std=c17 -Wall -Wextra -Werror   crates/platform-macos-sys/tests/fixtures/app_sandbox_child.c   -o "$BUILD/child"

cat > "$BUILD/parent.entitlements" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>com.apple.security.app-sandbox</key><true/>
  <key>com.apple.security.temporary-exception.files.absolute-path.read-only</key>
  <array>
    <string>${RO}/</string>
    <string>${BUILD}/</string>
  </array>
  <key>com.apple.security.temporary-exception.files.absolute-path.read-write</key>
  <array><string>${RW}/</string></array>
</dict></plist>
PLIST

cat > "$BUILD/child.entitlements" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>com.apple.security.app-sandbox</key><true/>
  <key>com.apple.security.inherit</key><true/>
</dict></plist>
PLIST

codesign --force --sign - --options runtime   --entitlements "$BUILD/parent.entitlements"   -i com.semwright.tests.app-sandbox-parent "$BUILD/parent"
codesign --force --sign - --options runtime   --entitlements "$BUILD/child.entitlements"   -i com.semwright.tests.app-sandbox-child "$BUILD/child"

codesign --verify --strict --verbose=2 "$BUILD/parent"
codesign --verify --strict --verbose=2 "$BUILD/child"

echo "running sandboxed parent smoke before child inheritance/path checks"
"$BUILD/parent" --smoke

python3 -m http.server 18765 --bind 127.0.0.1 --directory "$BUILD"   >"$BUILD/http.log" 2>&1 &
SERVER_PID=$!
for _ in {1..50}; do
  if curl --fail --silent --max-time 1 http://127.0.0.1:18765/ >/dev/null; then
    break
  fi
  sleep 0.1
done
curl --fail --silent --max-time 1 http://127.0.0.1:18765/ >/dev/null

"$BUILD/parent" "$BUILD/child" "$RO" "$RW" "$DENIED_ROOT" 18765

test "$(cat "$RW/output.txt")" = "written"
test ! -e "$RO/blocked.txt"

codesign -d --entitlements :- "$BUILD/parent" >"$BUILD/parent.entitlements.actual" 2>&1
codesign -d --entitlements :- "$BUILD/child" >"$BUILD/child.entitlements.actual" 2>&1
grep -q 'com.apple.security.app-sandbox' "$BUILD/parent.entitlements.actual"
grep -q 'com.apple.security.inherit' "$BUILD/child.entitlements.actual"

echo "macOS App Sandbox inherited-child probe: PASS"
