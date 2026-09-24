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
export AQ_NO_MODIFIERS=1
export AQ_TRACE=1
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
debug {
    disable_logs = false
}
windowrulev2 = float,title:^(Semwright Hyprland Fixture)$
CONF

# Aquamarine 0.15 binds wl_compositor v6 and requires linux-dmabuf. Weston 14
# advertises only wl_compositor v5, so use the same wlroots/Sway family already
# exercised by Semwright CI as a real headless parent compositor.
cat > "$home/sway-parent.conf" <<'SWAYCONF'
output * resolution 1280x720
seat seat0 fallback true
SWAYCONF
WLR_BACKENDS=headless WLR_RENDERER=gles2 WLR_LIBINPUT_NO_DEVICES=1 \
  sway --unsupported-gpu --config "$home/sway-parent.conf" --debug \
  >"$SEMWRIGHT_HYPRLAND_EVIDENCE_DIR/sway-parent.log" 2>&1 &
sway_pid=$!
parent_wayland=""
parent_sway=""
for _ in $(seq 1 200); do
  parent_wayland=$(find "$XDG_RUNTIME_DIR" -maxdepth 1 -type s -name "wayland-*" -printf "%f\n" -quit 2>/dev/null || true)
  parent_sway=$(find "$XDG_RUNTIME_DIR" -maxdepth 1 -type s -name "sway-ipc.*.sock" -print -quit 2>/dev/null || true)
  if [ -n "${parent_wayland:-}" ] && [ -n "${parent_sway:-}" ]; then
    break
  fi
  if ! kill -0 "$sway_pid" 2>/dev/null; then
    cat "$SEMWRIGHT_HYPRLAND_EVIDENCE_DIR/sway-parent.log"
    exit 1
  fi
  sleep 0.05
done
test -S "$XDG_RUNTIME_DIR/${parent_wayland:?parent Wayland socket missing}"
test -S "${parent_sway:?parent Sway IPC socket missing}"
SWAYSOCK="$parent_sway" swaymsg -t get_version | tee "$SEMWRIGHT_HYPRLAND_EVIDENCE_DIR/sway-parent-version.json"
export WAYLAND_DISPLAY="$parent_wayland"

Hyprland --config "$home/hyprland.conf" \
  >"$SEMWRIGHT_HYPRLAND_EVIDENCE_DIR/hyprland.log" 2>&1 &
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
  kill -TERM "$sway_pid" 2>/dev/null || true
  for _ in $(seq 1 40); do
    kill -0 "$sway_pid" 2>/dev/null || break
    sleep 0.05
  done
  kill -KILL "$sway_pid" 2>/dev/null || true
  wait "$sway_pid" 2>/dev/null || true
  internal_log=$(find "$runtime/hypr" -type f -name hyprland.log -print -quit 2>/dev/null || true)
  if [ -n "${internal_log:-}" ]; then
    cp "$internal_log" "$SEMWRIGHT_HYPRLAND_EVIDENCE_DIR/hyprland-internal.log" || true
  fi
  rm -rf "$runtime" "$home"
  exit "$status"
}
trap cleanup EXIT INT TERM

for _ in $(seq 1 200); do
  instance=$(find "$XDG_RUNTIME_DIR/hypr" -mindepth 1 -maxdepth 1 -type d -printf "%f\n" 2>/dev/null | head -1 || true)
  wayland=$(find "$XDG_RUNTIME_DIR" -maxdepth 1 -type s -name "wayland-*" ! -name "$parent_wayland" -printf "%f\n" 2>/dev/null | head -1 || true)
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
