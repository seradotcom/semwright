#!/usr/bin/env bash
set -euo pipefail

: "${SEMWRIGHT_HYPR_BASELINE_SHA:?SEMWRIGHT_HYPR_BASELINE_SHA is required}"

EVIDENCE_DIR=/evidence
BIN_DIR=${SEMWRIGHT_HYPR_BIN_DIR:-/test}
HOME_DIR=${SEMWRIGHT_HYPR_HOME:-/tmp/semwright-hypr-home}
RUNTIME_DIR=${SEMWRIGHT_HYPR_RUNTIME:-/run/semwright}
WAYLAND_PARENT=${SEMWRIGHT_HYPR_PARENT_SOCKET:-wayland-parent}

for command in kwin_wayland Hyprland hyprctl wayland-info foot python3; do
  command -v "$command" >/dev/null || {
    echo "missing Hyprland certification dependency: $command" >&2
    exit 5
  }
done
test -x "$BIN_DIR/semwright"
test -x "$BIN_DIR/semwrightd"

rm -rf "$HOME_DIR"
mkdir -p "$HOME_DIR" "$RUNTIME_DIR" "$EVIDENCE_DIR"
find "$RUNTIME_DIR" -mindepth 1 -maxdepth 1 -exec rm -rf -- {} +
chmod 700 "$HOME_DIR" "$RUNTIME_DIR"

export HOME="$HOME_DIR"
export XDG_RUNTIME_DIR="$RUNTIME_DIR"
export XDG_CONFIG_HOME="$HOME_DIR/.config"
export XDG_DATA_HOME="$HOME_DIR/.local/share"
export XDG_CACHE_HOME="$HOME_DIR/.cache"
export XDG_STATE_HOME="$HOME_DIR/.local/state"
export XDG_SESSION_TYPE=wayland
mkdir -p "$XDG_CONFIG_HOME" "$XDG_DATA_HOME" "$XDG_CACHE_HOME" "$XDG_STATE_HOME"

cat >"$EVIDENCE_DIR/hyprland.conf" <<'EOF'
monitor = ,800x600@60,0x0,1
input { kb_layout = us }
general { gaps_in = 0; gaps_out = 0; border_size = 1 }
animations { enabled = false }
misc {
    disable_hyprland_logo = true
    disable_splash_rendering = true
    force_default_wallpaper = 0
}
debug {
    disable_logs = false
    enable_stdout_logs = true
}
EOF

cat >"$EVIDENCE_DIR/policy.toml" <<'EOF'
[policy]
profile = "desktop"
allow = ["window.observe", "window.manage"]
confirm_mutations = false
EOF
chmod 0600 "$EVIDENCE_DIR/policy.toml"

printf '%s
' "$SEMWRIGHT_HYPR_BASELINE_SHA" >"$EVIDENCE_DIR/baseline-sha.txt"
kwin_wayland --version >"$EVIDENCE_DIR/kwin-version.log" 2>&1 || true
Hyprland --version >"$EVIDENCE_DIR/hyprland-version.log" 2>&1 || true

KWIN=""
HYPR=""
FOOT=""
DAEMON=""

cleanup() {
  local status=$?
  for pid in "$DAEMON" "$FOOT" "$HYPR" "$KWIN"; do
    if [[ -n "$pid" ]]; then
      kill -TERM "$pid" 2>/dev/null || true
    fi
  done
  sleep 0.2
  for pid in "$DAEMON" "$FOOT" "$HYPR" "$KWIN"; do
    if [[ -n "$pid" ]]; then
      kill -KILL "$pid" 2>/dev/null || true
    fi
  done
  exit "$status"
}
trap cleanup EXIT INT TERM

export XDG_CURRENT_DESKTOP=KDE
export KDE_FULL_SESSION=true
kwin_wayland   --virtual   --width 800   --height 600   --no-lockscreen   --socket "$WAYLAND_PARENT"   >"$EVIDENCE_DIR/kwin.log" 2>&1 &
KWIN=$!

for _ in $(seq 1 200); do
  [[ -S "$XDG_RUNTIME_DIR/$WAYLAND_PARENT" ]] && break
  kill -0 "$KWIN" 2>/dev/null || {
    cat "$EVIDENCE_DIR/kwin.log" >&2
    exit 10
  }
  sleep 0.05
done
test -S "$XDG_RUNTIME_DIR/$WAYLAND_PARENT"

export WAYLAND_DISPLAY="$WAYLAND_PARENT"
wayland-info >"$EVIDENCE_DIR/kwin-parent-wayland-info.txt" 2>&1

export XDG_CURRENT_DESKTOP=Hyprland
export HYPRLAND_NO_RT=1
Hyprland --config "$EVIDENCE_DIR/hyprland.conf" >"$EVIDENCE_DIR/hyprland.log" 2>&1 &
HYPR=$!

INSTANCE=""
for _ in $(seq 1 300); do
  kill -0 "$HYPR" 2>/dev/null || {
    tail -200 "$EVIDENCE_DIR/hyprland.log" >&2
    exit 11
  }
  IPC=$(find "$XDG_RUNTIME_DIR/hypr" -mindepth 2 -maxdepth 2 -type s -name ".socket.sock" 2>/dev/null | head -1 || true)
  if [[ -n "$IPC" ]]; then
    INSTANCE=$(basename "$(dirname "$IPC")")
    break
  fi
  sleep 0.05
done
test -n "$INSTANCE"
export HYPRLAND_INSTANCE_SIGNATURE="$INSTANCE"

CHILD=""
for _ in $(seq 1 100); do
  CHILD=$(find "$XDG_RUNTIME_DIR" -maxdepth 1 -type s -name "wayland-*" ! -name "$WAYLAND_PARENT" | head -1 || true)
  [[ -n "$CHILD" ]] && break
  sleep 0.05
done
test -n "$CHILD"
WAYLAND_DISPLAY=$(basename "$CHILD")
export WAYLAND_DISPLAY

hyprctl version >"$EVIDENCE_DIR/hyprland-version-live.log"
hyprctl monitors -j >"$EVIDENCE_DIR/hyprland-monitors-before.json"

foot   -T "Semwright Hyprland Live Fixture"   -a org.semwright.HyprFixture   --window-size-pixels=420x240   /bin/sh -c "sleep 300"   >"$EVIDENCE_DIR/foot.out" 2>"$EVIDENCE_DIR/foot.err" &
FOOT=$!

for _ in $(seq 1 200); do
  kill -0 "$FOOT" 2>/dev/null || {
    cat "$EVIDENCE_DIR/foot.err" >&2
    exit 12
  }
  hyprctl clients -j | grep -q "Semwright Hyprland Live Fixture" && break
  sleep 0.05
done

hyprctl clients -j >"$EVIDENCE_DIR/hypr-clients-tiled.json"
ADDR=$(python3 - <<'PY'
import json
d=json.load(open("/evidence/hypr-clients-tiled.json"))
w=next(x for x in d if x["title"]=="Semwright Hyprland Live Fixture")
print(w["address"])
PY
)
hyprctl dispatch togglefloating "address:$ADDR" >"$EVIDENCE_DIR/setup-floating.log"
sleep 0.15
hyprctl clients -j >"$EVIDENCE_DIR/hypr-clients-before.json"

"$BIN_DIR/semwrightd"   --config "$EVIDENCE_DIR/policy.toml"   --socket "$XDG_RUNTIME_DIR/semwright.sock"   --log-format json   >"$EVIDENCE_DIR/semwrightd.log" 2>&1 &
DAEMON=$!

for _ in $(seq 1 200); do
  [[ -S "$XDG_RUNTIME_DIR/semwright.sock" ]] && break
  kill -0 "$DAEMON" 2>/dev/null || {
    cat "$EVIDENCE_DIR/semwrightd.log" >&2
    exit 13
  }
  sleep 0.05
done
test -S "$XDG_RUNTIME_DIR/semwright.sock"

sem() {
  "$BIN_DIR/semwright"     --socket "$XDG_RUNTIME_DIR/semwright.sock"     --session-file "$HOME_DIR/client.session"     --json "$@"
}

sem doctor >"$EVIDENCE_DIR/doctor.json"
sem execute window.list --args-json "{}" >"$EVIDENCE_DIR/list1.json"
python3 - <<'PY'
import json
d=json.load(open("/evidence/list1.json"))
assert d["ok"] is True, d
assert d["execution"]["backend"] == "hyprland", d
w=next(x for x in d["data"]["windows"] if x["title"]=="Semwright Hyprland Live Fixture")
assert w["coordinate_space"]=="compositor_logical", w
open("/evidence/ref1.txt","w").write(w["ref"])
PY

ref=$(cat "$EVIDENCE_DIR/ref1.txt")
args=$(python3 -c 'import json,sys; print(json.dumps({"ref":sys.argv[1]}))' "$ref")
sem execute window.focus --args-json "$args" >"$EVIDENCE_DIR/focus.json"
sleep 0.15
sem execute window.list --args-json "{}" >"$EVIDENCE_DIR/list2.json"
python3 - <<'PY'
import json
d=json.load(open("/evidence/list2.json"))
w=next(x for x in d["data"]["windows"] if x["title"]=="Semwright Hyprland Live Fixture")
assert w["focused"] is True, w
open("/evidence/ref2.txt","w").write(w["ref"])
PY

ref=$(cat "$EVIDENCE_DIR/ref2.txt")
args=$(python3 -c 'import json,sys; print(json.dumps({"ref":sys.argv[1],"x":111,"y":77}))' "$ref")
sem execute window.move --args-json "$args" >"$EVIDENCE_DIR/move.json"
sleep 0.15
sem execute window.list --args-json "{}" >"$EVIDENCE_DIR/list3.json"
python3 - <<'PY'
import json
d=json.load(open("/evidence/list3.json"))
w=next(x for x in d["data"]["windows"] if x["title"]=="Semwright Hyprland Live Fixture")
assert w["position"] == [111,77], w
open("/evidence/ref3.txt","w").write(w["ref"])
PY

ref=$(cat "$EVIDENCE_DIR/ref3.txt")
args=$(python3 -c 'import json,sys; print(json.dumps({"ref":sys.argv[1],"width":500,"height":320}))' "$ref")
sem execute window.resize --args-json "$args" >"$EVIDENCE_DIR/resize.json"
sleep 0.15
sem execute window.list --args-json "{}" >"$EVIDENCE_DIR/list4.json"
python3 - <<'PY'
import json
d=json.load(open("/evidence/list4.json"))
w=next(x for x in d["data"]["windows"] if x["title"]=="Semwright Hyprland Live Fixture")
assert w["size"] == [500,320], w
open("/evidence/ref4.txt","w").write(w["ref"])
PY

oldref=$(cat "$EVIDENCE_DIR/ref4.txt")
kill -TERM "$FOOT"
wait "$FOOT" 2>/dev/null || true
FOOT=""

for _ in $(seq 1 100); do
  if ! hyprctl clients -j | grep -q "Semwright Hyprland Live Fixture"; then
    break
  fi
  sleep 0.05
done

set +e
args=$(python3 -c 'import json,sys; print(json.dumps({"ref":sys.argv[1]}))' "$oldref")
sem execute window.focus --args-json "$args" >"$EVIDENCE_DIR/stale.json"
STALE_RC=$?
set -e
python3 - <<'PY'
import json
d=json.load(open("/evidence/stale.json"))
assert d["ok"] is False, d
assert d["error"]["code"] == "StaleReference", d
PY
test "$STALE_RC" -ne 0
hyprctl clients -j >"$EVIDENCE_DIR/hypr-clients-after.json"

hyprctl output create headless semwright-extra >"$EVIDENCE_DIR/multimon-create.log"
for _ in $(seq 1 100); do
  hyprctl monitors -j >"$EVIDENCE_DIR/multimon-created.json"
  grep -q '"name": "semwright-extra"' "$EVIDENCE_DIR/multimon-created.json" && break
  sleep 0.05
done
hyprctl keyword monitor "semwright-extra,640x480@60,800x0,1.25" >"$EVIDENCE_DIR/multimon-keyword.log"
sleep 0.4
hyprctl monitors -j >"$EVIDENCE_DIR/multimon-final.json"

python3 - <<'PY'
import json, pathlib
root=pathlib.Path("/evidence")
monitors=json.load(open(root/"multimon-final.json"))
assert len(monitors) >= 2, monitors
extra=next(x for x in monitors if x["name"]=="semwright-extra")
assert (extra["x"], extra["y"]) == (800,0), extra
assert (extra["width"], extra["height"]) == (640,480), extra
assert abs(extra["scale"]-1.25) < 1e-6, extra
assert any(abs(x["scale"]-1.0) < 1e-6 for x in monitors if x["name"]!="semwright-extra"), monitors

checks={}
for name in ("focus","move","resize"):
    d=json.load(open(root/f"{name}.json"))
    assert d["ok"] is True and d["execution"]["backend"]=="hyprland", d
    checks[name]={
        "ok": True,
        "backend": "hyprland",
        "coordinate_space": d["data"].get("coordinate_space"),
    }
doctor=json.load(open(root/"doctor.json"))
hypr=next(x for x in doctor["data"]["features"] if x["backend"]=="hyprland")
assert hypr["status"]=="SUPPORTED", hypr
after=json.load(open(root/"hypr-clients-after.json"))
assert after == [], after

summary={
    "status":"PASS",
    "baseline_sha":(root/"baseline-sha.txt").read_text().strip(),
    "backend":"hyprland",
    "doctor":hypr,
    "checks":checks,
    "stale_reference":"PASS",
    "fixture_cleanup":"PASS",
    "multi_monitor_scale":{
        "status":"PASS",
        "primary":{"name":monitors[0]["name"],"scale":monitors[0]["scale"]},
        "secondary":{
            "name":extra["name"],
            "position":[extra["x"],extra["y"]],
            "size":[extra["width"],extra["height"]],
            "scale":extra["scale"],
        },
    },
}
(root/"summary.json").write_text(json.dumps(summary, indent=2, sort_keys=True) + chr(10))
print(json.dumps(summary,indent=2,sort_keys=True))
PY
