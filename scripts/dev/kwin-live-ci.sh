#!/usr/bin/env bash
set -euo pipefail

: "${SEMWRIGHT_KWIN_TEST_BIN:?SEMWRIGHT_KWIN_TEST_BIN is required}"
EVIDENCE_DIR=${SEMWRIGHT_KWIN_EVIDENCE_DIR:-/evidence}
HOME_DIR=${SEMWRIGHT_KWIN_HOME:-/tmp/semwright-plasma-home}
RUNTIME_DIR=${SEMWRIGHT_KWIN_RUNTIME:-/tmp/semwright-plasma-runtime}
WAYLAND_SOCKET=${SEMWRIGHT_KWIN_WAYLAND_DISPLAY:-semwright-plasma}

for command in dbus-run-session dbus-update-activation-environment gdbus kwin_wayland kpackagetool6 kwriteconfig6 kdialog; do
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
# The output of KWin is a private host Xvfb; window fixtures are real
# Wayland clients of KWin's separate, owner-only socket.
: "${DISPLAY:?private Xvfb output display is required}"
: "${XAUTHORITY:?private Xvfb authorization file is required}"
test -r "$XAUTHORITY"
# This fixture tests native KWin window management, not desktop document
# portals. Do not activate a FUSE-backed file portal in a network-isolated,
# unprivileged container; it is unrelated to the compositor contract.
export QT_NO_XDG_DESKTOP_PORTAL=1
export GDK_BACKEND=wayland
# D-Bus activates KDE/GTK portal helpers as separate processes. They need the
# same private Wayland socket as our clients; exporting to this shell alone does
# not update the already-running session bus's activation environment.
dbus-update-activation-environment \
  WAYLAND_DISPLAY QT_QPA_PLATFORM XDG_SESSION_TYPE XDG_CURRENT_DESKTOP XDG_RUNTIME_DIR \
  QT_NO_XDG_DESKTOP_PORTAL GDK_BACKEND \
  >"$EVIDENCE_DIR/dbus-activation.log" 2>&1
export LIBGL_ALWAYS_SOFTWARE=1
# Hosted CI has no DRM render device. Keep a real KWin compositor running
# with its QPainter compositor and Qt Quick software scene graph.
export KWIN_COMPOSE=Q
export QT_QUICK_BACKEND=software
printf 'KWIN_OUTPUT=NESTED_XVFB KWIN_RENDERER=QPainter QT_QUICK_BACKEND=software\n' > "$EVIDENCE_DIR/render-mode.log"

{
  kwin_wayland --version
  kpackagetool6 --version
  # kdialog is a GUI binary and initializes Qt/Wayland even for --version.
  # Record the installed package version before KWin starts without invoking it.
  dpkg-query -W kdialog
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

# KWin is the Wayland server nested on the private Xvfb output, NOT a
# client of its own Wayland socket. Only the fixture/test use that socket.
# Permission relaxation applies only to this private, network-isolated CI compositor.
env -u WAYLAND_DISPLAY -u QT_QPA_PLATFORM -u KDE_FULL_SESSION -u KDE_SESSION_VERSION \
  -u QT_NO_XDG_DESKTOP_PORTAL \
  KWIN_WAYLAND_NO_PERMISSION_CHECKS=1 \
  QT_LOGGING_RULES='kwin_core.debug=true;kwin_wayland.debug=true;kwin_scripting.debug=true' \
  kwin_wayland   --x11-display "$DISPLAY"   --width 1280   --height 720   --no-lockscreen   --socket "$WAYLAND_SOCKET"   >"$EVIDENCE_DIR/kwin.log" 2>&1 &
KWIN_PID=$!

set +e
wait "$TEST_PID"
TEST_RC=$?
TEST_PID=""
set -e
cat "$EVIDENCE_DIR/kwin-rust-test.log"
test "$TEST_RC" -eq 0

gdbus call --session   --dest org.freedesktop.DBus   --object-path /org/freedesktop/DBus   --method org.freedesktop.DBus.NameHasOwner org.kde.KWin   | tee "$EVIDENCE_DIR/kwin-bus-owner.log" | grep -q true

printf '%s\n' "Plasma/KWin nested Wayland integration: PASS"   | tee "$EVIDENCE_DIR/result.log"
