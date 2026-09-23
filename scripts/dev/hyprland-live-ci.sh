#!/usr/bin/env bash
set -euo pipefail

: "${SEMWRIGHT_HYPRLAND_TEST_BIN:?test binary path required}"
: "${SEMWRIGHT_HYPRLAND_EVIDENCE_DIR:?evidence directory required}"

mkdir -p "$SEMWRIGHT_HYPRLAND_EVIDENCE_DIR"
runtime=$(mktemp -d)
home=$(mktemp -d)
chmod 700 "$runtime" "$home"
export XDG_RUNTIME_DIR="$runtime"
export HOME="$home"
export XDG_SESSION_TYPE=wayland
export XDG_CURRENT_DESKTOP=Hyprland
export XDG_SESSION_DESKTOP=Hyprland
export AQ_NO_KMS_REQUIREMENT=1
export HYPRLAND_NO_RT=1
export HYPRLAND_NO_SD_NOTIFY=1
export HYPRLAND_NO_SD_VARS=1
export LIBGL_ALWAYS_SOFTWARE=1
export SEMWRIGHT_TEST_HYPRLAND=1

cat > "$home/hyprland.conf" <<'CONF'
monitor=,preferred,auto,1
misc {
    disable_hyprland_logo = true
    disable_splash_rendering = true
}
windowrulev2 = float,title:^(Semwright Hyprland Fixture)$
CONF

Hyprland --config "$home/hyprland.conf"   >"$SEMWRIGHT_HYPRLAND_EVIDENCE_DIR/hyprland.log" 2>&1 &
hyprland_pid=$!
cleanup() {
  status=$?
  kill -TERM "$hyprland_pid" 2>/dev/null || true
  for _ in $(seq 1 60); do
    kill -0 "$hyprland_pid" 2>/dev/null || break
    sleep 0.05
  done
  kill -KILL "$hyprland_pid" 2>/dev/null || true
  wait "$hyprland_pid" 2>/dev/null || true
  rm -rf "$runtime" "$home"
  exit "$status"
}
trap cleanup EXIT INT TERM

for _ in $(seq 1 200); do
  instance=$(find "$XDG_RUNTIME_DIR/hypr" -mindepth 1 -maxdepth 1 -type d -printf "%f\n" 2>/dev/null | head -1 || true)
  wayland=$(find "$XDG_RUNTIME_DIR" -maxdepth 1 -type s -name "wayland-*" -printf "%f\n" 2>/dev/null | head -1 || true)
  if [ -n "${instance:-}" ] && [ -n "${wayland:-}" ]; then
    export HYPRLAND_INSTANCE_SIGNATURE="$instance"
    export WAYLAND_DISPLAY="$wayland"
    break
  fi
  if ! kill -0 "$hyprland_pid" 2>/dev/null; then
    cat "$SEMWRIGHT_HYPRLAND_EVIDENCE_DIR/hyprland.log"
    exit 1
  fi
  sleep 0.05
done

test -n "${HYPRLAND_INSTANCE_SIGNATURE:-}"
test -S "$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/.socket.sock"
test -S "$XDG_RUNTIME_DIR/${WAYLAND_DISPLAY:?Wayland socket missing}"

hyprctl -j version | tee "$SEMWRIGHT_HYPRLAND_EVIDENCE_DIR/hyprctl-version.json"
hyprctl -j monitors | tee "$SEMWRIGHT_HYPRLAND_EVIDENCE_DIR/hyprctl-monitors.json"
if [ "$(hyprctl -j monitors | python3 -c 'import json,sys; print(len(json.load(sys.stdin)))')" -eq 0 ]; then
  hyprctl output create headless semwright-ci
  sleep 0.2
  hyprctl -j monitors | tee "$SEMWRIGHT_HYPRLAND_EVIDENCE_DIR/hyprctl-monitors-after-headless.json"
fi

timeout --signal=TERM --kill-after=5s 90s   "$SEMWRIGHT_HYPRLAND_TEST_BIN"     real_hyprland_window_lifecycle_uses_native_socket     --ignored --nocapture --test-threads=1   2>&1 | tee "$SEMWRIGHT_HYPRLAND_EVIDENCE_DIR/hyprland-backend.log"
