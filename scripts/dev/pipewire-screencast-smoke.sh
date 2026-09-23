#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
cd "$ROOT"

for command in pipewire pw-dump gst-launch-1.0 python3 cargo; do
  command -v "$command" >/dev/null || {
    echo "missing dependency: $command" >&2
    exit 5
  }
done

TMP=$(mktemp -d)
PIPEWIRE_PID=""
WIREPLUMBER_PID=""
GST_PID=""
cleanup() {
  for pid in "$GST_PID" "$WIREPLUMBER_PID" "$PIPEWIRE_PID"; do
    if [[ -n "$pid" ]]; then
      kill -TERM "$pid" 2>/dev/null || true
      wait "$pid" 2>/dev/null || true
    fi
  done
  rm -rf "$TMP"
}
trap cleanup EXIT

export XDG_RUNTIME_DIR="$TMP/runtime"
mkdir -p "$XDG_RUNTIME_DIR"
chmod 700 "$TMP" "$XDG_RUNTIME_DIR"
export PIPEWIRE_REMOTE=pipewire-0

pipewire >"$TMP/pipewire.log" 2>&1 &
PIPEWIRE_PID=$!
for _ in $(seq 1 160); do
  [[ -S "$XDG_RUNTIME_DIR/pipewire-0" ]] && break
  if ! kill -0 "$PIPEWIRE_PID" 2>/dev/null; then
    cat "$TMP/pipewire.log" >&2
    exit 1
  fi
  sleep 0.05
done
[[ -S "$XDG_RUNTIME_DIR/pipewire-0" ]] || {
  cat "$TMP/pipewire.log" >&2
  exit 1
}

if command -v wireplumber >/dev/null; then
  wireplumber >"$TMP/wireplumber.log" 2>&1 &
  WIREPLUMBER_PID=$!
  sleep 0.5
fi

gst-launch-1.0 -q   videotestsrc is-live=true pattern=smpte   ! video/x-raw,format=BGRA,width=64,height=48,framerate=30/1   ! pipewiresink >"$TMP/gstreamer.log" 2>&1 &
GST_PID=$!

TARGET="$TMP/target.env"
for _ in $(seq 1 200); do
  if ! kill -0 "$GST_PID" 2>/dev/null; then
    cat "$TMP/gstreamer.log" >&2
    exit 1
  fi
  pw-dump >"$TMP/pw-dump.json" 2>"$TMP/pw-dump.err" || true
  if python3 - "$TMP/pw-dump.json" "$TARGET" <<'PY'
import json, sys
src, dst = sys.argv[1:]
rows = json.load(open(src))
for row in rows:
    if row.get("type") != "PipeWire:Interface:Node":
        continue
    props = row.get("info", {}).get("props", {})
    media = props.get("media.class", "")
    app = str(props.get("application.name", ""))
    name = str(props.get("node.name", ""))
    serial = props.get("object.serial")
    if (media == "Stream/Output/Video" or "gst-launch" in app or "gst-launch" in name) and serial:
        with open(dst, "w") as handle:
            handle.write("SEMWRIGHT_TEST_PIPEWIRE_NODE=%s\n" % int(row["id"]))
            handle.write("SEMWRIGHT_TEST_PIPEWIRE_SERIAL=%s\n" % int(serial))
        raise SystemExit(0)
raise SystemExit(1)
PY
  then
    break
  fi
  sleep 0.05
done

[[ -s "$TARGET" ]] || {
  cat "$TMP/pw-dump.json" >&2 || true
  cat "$TMP/gstreamer.log" >&2 || true
  exit 1
}
source "$TARGET"
export SEMWRIGHT_TEST_PIPEWIRE=1
export SEMWRIGHT_TEST_PIPEWIRE_SOCKET="$XDG_RUNTIME_DIR/pipewire-0"
export SEMWRIGHT_TEST_PIPEWIRE_NODE
export SEMWRIGHT_TEST_PIPEWIRE_SERIAL
printf 'target_node=%s target_serial=%s\n' "$SEMWRIGHT_TEST_PIPEWIRE_NODE" "$SEMWRIGHT_TEST_PIPEWIRE_SERIAL"

cargo test --locked -p semwright-platform-linux   --test pipewire_live   real_pipewire_capture_negotiates_frame_png_and_cancellation   -- --ignored --nocapture

echo "PipeWire synthetic frame capture: PASS"
