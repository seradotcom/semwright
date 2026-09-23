#!/usr/bin/env bash
set -euo pipefail

: "${SEMWRIGHT_KWIN_TEST_BIN:?SEMWRIGHT_KWIN_TEST_BIN is required}"
EVIDENCE_DIR=${SEMWRIGHT_KWIN_EVIDENCE_DIR:-/evidence}
HOME_DIR=${SEMWRIGHT_KWIN_HOME:-/tmp/semwright-plasma-home}
RUNTIME_DIR=${SEMWRIGHT_KWIN_RUNTIME:-/tmp/semwright-plasma-runtime}
WAYLAND_SOCKET=${SEMWRIGHT_KWIN_WAYLAND_DISPLAY:-semwright-plasma}

for command in dbus-run-session gdbus kwin_wayland kpackagetool6 kwriteconfig6 kdialog; do
  command -v "$command" >/dev/null || {
    echo "missing Plasma test dependency: $command" >&2
    exit 5
  }
done
test -x "$SEMWRIGHT_KWIN_TEST_BIN"
test -d /workspace/bridges/kwin

rm -rf "$HOME_DIR" "$RUNTIME_DIR"
mkdir -p "$HOME_DIR" "$RUNTIME_DIR" "$EVIDENCE_DIR"
chmod 700 "$HOME_DIR" "$RUNTIME_DIR"

export HOME="$HOME_DIR"
export XDG_RUNTIME_DIR="$RUNTIME_DIR"
export XDG_CONFIG_HOME="$HOME_DIR/.config"
export XDG_DATA_HOME="$HOME_DIR/.local/share"
export XDG_CACHE_HOME="$HOME_DIR/.cache"
export XDG_SESSION_TYPE=wayland
export XDG_CURRENT_DESKTOP=KDE
export KDE_FULL_SESSION=true
export QT_QPA_PLATFORM=wayland
export WAYLAND_DISPLAY="$WAYLAND_SOCKET"
export LIBGL_ALWAYS_SOFTWARE=1

{
  kwin_wayland --version
  kpackagetool6 --version
  # kdialog is a GUI binary and initializes Qt/Wayland even for --version.
  # Record the installed package version before KWin starts without invoking it.
  dpkg-query -W -f=kdialog=
 kdialog
} | tee "$EVIDENCE_DIR/plasma-versions.log"

kpackagetool6 --type=KWin/Script -i /workspace/bridges/kwin   2>&1 | tee "$EVIDENCE_DIR/kwin-package-install.log"
kwriteconfig6 --file kwinrc --group Plugins --key semwrightEnabled true

cleanup() {
  local status=$?
  if [[ -n "${KWIN_PID:-}" ]]; then
    kill -TERM "$KWIN_PID" 2>/dev/null || true
    for _ in $(seq 1 50); do
      kill -0 "$KWIN_PID" 2>/dev/null || break
      sleep 0.05
    done
    kill -KILL "$KWIN_PID" 2>/dev/null || true
    wait "$KWIN_PID" 2>/dev/null || true
  fi
  if [[ -n "${TEST_PID:-}" ]]; then
    kill -TERM "$TEST_PID" 2>/dev/null || true
    wait "$TEST_PID" 2>/dev/null || true
  fi
  if [[ -f "$EVIDENCE_DIR/kwin.log" ]]; then
    tail -200 "$EVIDENCE_DIR/kwin.log" || true
  fi
  exit "$status"
}
trap cleanup EXIT INT TERM

SEMWRIGHT_TEST_KWIN=1 SEMWRIGHT_TEST_KWIN_FIXTURE=/usr/bin/kdialog   "$SEMWRIGHT_KWIN_TEST_BIN"     --exact real_kwin6_mailbox_controls_window_and_rejects_stale_ref     --ignored --nocapture     >"$EVIDENCE_DIR/kwin-rust-test.log" 2>&1 &
TEST_PID=$!

for _ in $(seq 1 100); do
  if gdbus call --session       --dest org.freedesktop.DBus       --object-path /org/freedesktop/DBus       --method org.freedesktop.DBus.NameHasOwner org.semwright.Broker       2>/dev/null | grep -q true; then
    break
  fi
  kill -0 "$TEST_PID" 2>/dev/null || {
    cat "$EVIDENCE_DIR/kwin-rust-test.log" >&2
    exit 1
  }
  sleep 0.05
done

gdbus call --session   --dest org.freedesktop.DBus   --object-path /org/freedesktop/DBus   --method org.freedesktop.DBus.NameHasOwner org.semwright.Broker   | tee "$EVIDENCE_DIR/broker-bus-owner.log" | grep -q true

kwin_wayland   --virtual   --width 1280   --height 720   --no-lockscreen   --socket "$WAYLAND_SOCKET"   >"$EVIDENCE_DIR/kwin.log" 2>&1 &
KWIN_PID=$!

set +e
wait "$TEST_PID"
TEST_RC=$?
TEST_PID=""
set -e
cat "$EVIDENCE_DIR/kwin-rust-test.log"
test "$TEST_RC" -eq 0

gdbus call --session   --dest org.freedesktop.DBus   --object-path /org/freedesktop/DBus   --method org.freedesktop.DBus.NameHasOwner org.kde.KWin   | tee "$EVIDENCE_DIR/kwin-bus-owner.log" | grep -q true

printf '%s\n' "Plasma/KWin virtual Wayland integration: PASS"   | tee "$EVIDENCE_DIR/result.log"
