#!/usr/bin/env bash
set -euo pipefail

ACK_EXPECTED="I_AM_ON_A_DISPOSABLE_PHYSICAL_HYPRLAND_LOGIN"

fail() {
  echo "hyprland physical preflight: $*" >&2
  exit 2
}

[[ "${SEMWRIGHT_HYPR_PHYSICAL_ACK:-}" == "$ACK_EXPECTED" ]] ||   fail "set SEMWRIGHT_HYPR_PHYSICAL_ACK=$ACK_EXPECTED after confirming a disposable physical Hyprland login"

for command in hyprctl loginctl python3; do
  command -v "$command" >/dev/null || fail "missing dependency: $command"
done

[[ "${XDG_SESSION_TYPE:-}" == "wayland" ]] || fail "XDG_SESSION_TYPE must be wayland"
desktop="${XDG_CURRENT_DESKTOP:-${XDG_SESSION_DESKTOP:-}}"
shopt -s nocasematch
[[ "$desktop" == *hyprland* ]] || fail "desktop is not Hyprland: ${desktop:-unset}"
shopt -u nocasematch
[[ -n "${HYPRLAND_INSTANCE_SIGNATURE:-}" ]] || fail "HYPRLAND_INSTANCE_SIGNATURE is missing"

session_id="${SEMWRIGHT_HYPR_SESSION_ID:-${XDG_SESSION_ID:-}}"
if [[ -z "$session_id" ]]; then
  session_id=$(loginctl show-user "$UID" -p Display --value)
fi
[[ -n "$session_id" ]] || fail "could not resolve the active logind session"

active=$(loginctl show-session "$session_id" -p Active --value)
session_type=$(loginctl show-session "$session_id" -p Type --value)
remote=$(loginctl show-session "$session_id" -p Remote --value)
class=$(loginctl show-session "$session_id" -p Class --value)
state=$(loginctl show-session "$session_id" -p State --value)

[[ "$active" == "yes" ]] || fail "logind session is not active"
[[ "$session_type" == "wayland" ]] || fail "logind session type is not wayland"
[[ "$remote" == "no" ]] || fail "remote logind sessions are not physical certification targets"
[[ "$class" == "user" ]] || fail "logind session class is not user"
[[ "$state" == "active" ]] || fail "logind session state is not active"

compositor_pid="${SEMWRIGHT_HYPR_COMPOSITOR_PID:-}"
if [[ -z "$compositor_pid" ]]; then
  command -v pgrep >/dev/null || fail "missing dependency: pgrep"
  compositor_pid=$(pgrep -u "$UID" -x Hyprland | head -n1 || true)
fi
[[ "$compositor_pid" =~ ^[0-9]+$ ]] || fail "could not resolve Hyprland compositor pid"

proc_root="${SEMWRIGHT_PROC_ROOT:-/proc}"
environ_file="$proc_root/$compositor_pid/environ"
[[ -r "$environ_file" ]] || fail "cannot read compositor environment: $environ_file"
if tr '\0' '\n' < "$environ_file" | grep -q '^WAYLAND_DISPLAY='; then
  fail "Hyprland compositor inherited WAYLAND_DISPLAY; this looks nested, not a physical login"
fi

monitors_json=$(hyprctl monitors -j)
clients_json=$(hyprctl clients -j)

SEMWRIGHT_HYPR_MONITORS_JSON="$monitors_json" SEMWRIGHT_HYPR_CLIENTS_JSON="$clients_json" SEMWRIGHT_HYPR_SESSION_ID_RESOLVED="$session_id" SEMWRIGHT_HYPR_DESKTOP_RESOLVED="$desktop" SEMWRIGHT_HYPR_REQUIRE_MIXED_SCALE_RESOLVED="${SEMWRIGHT_HYPR_REQUIRE_MIXED_SCALE:-0}" python3 - <<'PY'
import json
import os
import re

monitors = json.loads(os.environ["SEMWRIGHT_HYPR_MONITORS_JSON"])
clients = json.loads(os.environ["SEMWRIGHT_HYPR_CLIENTS_JSON"])
if not isinstance(monitors, list) or not monitors:
    raise SystemExit("hyprland physical preflight: no monitors reported")
if not isinstance(clients, list):
    raise SystemExit("hyprland physical preflight: clients payload is not a list")

physical_pattern = re.compile(r"^(?:eDP|DP|HDMI|DVI|VGA|LVDS|DSI)(?:[-A-Za-z0-9_.]*)$")
physical = [m for m in monitors if physical_pattern.match(str(m.get("name", "")))]
if not physical:
    raise SystemExit(
        "hyprland physical preflight: no physical connector-like monitor found; "
        "headless/nested outputs are not certification evidence"
    )

scales = sorted({float(m.get("scale", 1.0)) for m in physical})
require_mixed = os.environ["SEMWRIGHT_HYPR_REQUIRE_MIXED_SCALE_RESOLVED"] == "1"
if require_mixed and (len(physical) < 2 or len(scales) < 2):
    raise SystemExit(
        "hyprland physical preflight: mixed-scale evidence requested but two "
        "physical outputs with distinct scales were not observed"
    )

result = {
    "status": "PASS_PREFLIGHT_ONLY",
    "certification_complete": False,
    "session_id": os.environ["SEMWRIGHT_HYPR_SESSION_ID_RESOLVED"],
    "desktop": os.environ["SEMWRIGHT_HYPR_DESKTOP_RESOLVED"],
    "physical_monitor_names": [str(m.get("name")) for m in physical],
    "physical_monitor_count": len(physical),
    "physical_scales": scales,
    "mixed_scale_observed": len(scales) > 1,
    "client_count": len(clients),
    "next_step": (
        "Run the physical Hyprland live-cert procedure with disposable fixtures; "
        "this preflight alone is not R06 closure evidence."
    ),
}
print(json.dumps(result, indent=2, sort_keys=True))
PY
