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
APP="$BUILD/SemwrightSandboxProbe.app"
APP_EXEC="$APP/Contents/MacOS/SemwrightSandboxProbe"
HELPER="$APP/Contents/Helpers/semwright-sandbox-child"
EXEC_WRAPPER="$APP/Contents/Helpers/semwright-sandbox-exec"
PAYLOAD="$BUILD/pinned-payload"
RO_PAYLOAD="$RO/not-executable-by-policy"
mkdir -p "$RO" "$RW" "$BUILD" "$DENIED_ROOT" "$APP/Contents/MacOS" "$APP/Contents/Helpers"
cleanup() {
  kill "${SERVER_PID:-}" 2>/dev/null || true
  chmod -R u+w "$ROOT" "$DENIED_ROOT" 2>/dev/null || true
  rm -rf "$ROOT" "$DENIED_ROOT"
}
trap cleanup EXIT

printf 'allowed-ro' > "$RO/input.txt"
printf 'denied' > "$DENIED_ROOT/secret.txt"
chmod 0700 "$RW" "$DENIED_ROOT"

cc -std=c17 -Wall -Wextra -Werror \
  crates/platform-macos-sys/tests/fixtures/app_sandbox_parent.c -o "$APP_EXEC"
cc -std=c17 -Wall -Wextra -Werror \
  crates/platform-macos-sys/tests/fixtures/app_sandbox_child.c -o "$HELPER"
cc -std=c17 -Wall -Wextra -Werror \
  crates/platform-macos-sys/tests/fixtures/app_sandbox_exec_wrapper.c -o "$EXEC_WRAPPER"
cc -std=c17 -Wall -Wextra -Werror \
  crates/platform-macos-sys/tests/fixtures/app_sandbox_child.c -o "$PAYLOAD"
cc -std=c17 -Wall -Wextra -Werror \
  crates/platform-macos-sys/tests/fixtures/app_sandbox_exec_marker.c -o "$RO_PAYLOAD"
PAYLOAD_SHA_BEFORE="$(shasum -a 256 "$PAYLOAD" | awk '{print $1}')"
RO_PAYLOAD_SHA_BEFORE="$(shasum -a 256 "$RO_PAYLOAD" | awk '{print $1}')"
chmod 0555 "$RO"

cat > "$APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleIdentifier</key><string>com.semwright.tests.app-sandbox-parent</string>
  <key>CFBundleExecutable</key><string>SemwrightSandboxProbe</string>
  <key>CFBundleName</key><string>SemwrightSandboxProbe</string>
  <key>CFBundlePackageType</key><string>APPL</string>
</dict></plist>
PLIST

cat > "$BUILD/parent.entitlements" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>com.apple.security.app-sandbox</key><true/>
  <key>com.apple.security.temporary-exception.files.absolute-path.read-only</key>
  <array>
    <string>${RO}/</string>
    <string>${PAYLOAD}</string>
  </array>
  <key>com.apple.security.temporary-exception.files.absolute-path.read-write</key>
  <array><string>${RW}/</string></array>
</dict></plist>
PLIST

cat > "$BUILD/parent.sandbox-only.entitlements" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>com.apple.security.app-sandbox</key><true/>
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

run_parent_smoke() {
  local phase="$1"
  local rc=0
  set +e
  "$APP_EXEC" --smoke
  rc=$?
  set -e
  echo "sandbox-parent-smoke phase=$phase rc=$rc"
  return "$rc"
}

codesign --force --sign - --options runtime \
  --entitlements "$BUILD/child.entitlements" \
  -i com.semwright.tests.app-sandbox-child "$HELPER"
codesign --verify --strict --verbose=2 "$HELPER"
codesign --force --sign - --options runtime \
  --entitlements "$BUILD/child.entitlements" \
  -i com.semwright.tests.app-sandbox-exec "$EXEC_WRAPPER"
codesign --verify --strict --verbose=2 "$EXEC_WRAPPER"

codesign --force --sign - --options runtime \
  -i com.semwright.tests.app-sandbox-parent "$APP"
codesign --verify --strict --verbose=2 "$APP"
BASE_RC=0
run_parent_smoke adhoc-no-entitlements || BASE_RC=$?

codesign --force --sign - --options runtime \
  --entitlements "$BUILD/parent.sandbox-only.entitlements" \
  -i com.semwright.tests.app-sandbox-parent "$APP"
codesign --verify --strict --verbose=2 "$APP"
SANDBOX_RC=0
run_parent_smoke app-sandbox-only || SANDBOX_RC=$?

codesign --force --sign - --options runtime \
  --entitlements "$BUILD/parent.entitlements" \
  -i com.semwright.tests.app-sandbox-parent "$APP"
codesign --verify --strict --verbose=2 "$APP"
PATH_RC=0
run_parent_smoke app-sandbox-with-path-exceptions || PATH_RC=$?

if (( BASE_RC != 0 )); then
  echo "ad-hoc signed parent cannot launch; App Sandbox probe is inconclusive" >&2
  exit 70
fi
if (( SANDBOX_RC != 0 )); then
  echo "app-sandbox entitlement aborts the ad-hoc signed parent before child inheritance" >&2
  exit 71
fi
if (( PATH_RC != 0 )); then
  echo "absolute-path temporary exceptions abort the otherwise sandboxed parent" >&2
  exit 72
fi

python3 -m http.server 18765 --bind 127.0.0.1 --directory "$BUILD"   >"$BUILD/http.log" 2>&1 &
SERVER_PID=$!
for _ in {1..50}; do
  if curl --fail --silent --max-time 1 http://127.0.0.1:18765/ >/dev/null; then
    break
  fi
  sleep 0.1
done
curl --fail --silent --max-time 1 http://127.0.0.1:18765/ >/dev/null

"$APP_EXEC" "$HELPER" "$RO" "$RW" "$DENIED_ROOT" 18765
test "$(cat "$RW/output.txt")" = "written"
test ! -e "$RO/blocked.txt"

rm "$RW/output.txt"
"$APP_EXEC" "$EXEC_WRAPPER" "$PAYLOAD" "$RO" "$RW" "$DENIED_ROOT" 18765
test "$(cat "$RW/output.txt")" = "written"
test ! -e "$RO/blocked.txt"
PAYLOAD_SHA_AFTER="$(shasum -a 256 "$PAYLOAD" | awk '{print $1}')"
test "$PAYLOAD_SHA_AFTER" = "$PAYLOAD_SHA_BEFORE"
echo "macOS App Sandbox exec-pinned-payload probe: PASS sha256=$PAYLOAD_SHA_AFTER"

rm -f "$RW/exec-marker.txt"
set +e
"$APP_EXEC" "$EXEC_WRAPPER" "$RO_PAYLOAD" "$RO" "$RW" "$DENIED_ROOT" 18765
RO_EXEC_RC=$?
set -e
RO_PAYLOAD_SHA_AFTER="$(shasum -a 256 "$RO_PAYLOAD" | awk '{print $1}')"
test "$RO_PAYLOAD_SHA_AFTER" = "$RO_PAYLOAD_SHA_BEFORE"
if [[ -e "$RW/exec-marker.txt" || "$RO_EXEC_RC" -eq 0 ]]; then
  echo "read-only App Sandbox root permitted execution without Semwright execute authority" >&2
  exit 73
fi
echo "macOS App Sandbox read-only-root execute denial: PASS rc=$RO_EXEC_RC"

codesign -d --entitlements :- "$APP" >"$BUILD/parent.entitlements.actual" 2>&1
codesign -d --entitlements :- "$HELPER" >"$BUILD/child.entitlements.actual" 2>&1
grep -q 'com.apple.security.app-sandbox' "$BUILD/parent.entitlements.actual"
grep -q 'com.apple.security.inherit' "$BUILD/child.entitlements.actual"

echo "macOS App Sandbox inherited-child probe: PASS"
