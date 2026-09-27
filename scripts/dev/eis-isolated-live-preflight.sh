#!/usr/bin/env bash
set -euo pipefail

ACK_EXPECTED="I_AM_IN_A_DISPOSABLE_VM_OR_INDEPENDENT_SEAT"

fail() {
  echo "EIS isolated live preflight: $*" >&2
  exit 2
}

[[ "${SEMWRIGHT_EIS_ISOLATION_ACK:-}" == "$ACK_EXPECTED" ]] ||   fail "set SEMWRIGHT_EIS_ISOLATION_ACK=$ACK_EXPECTED only inside the disposable certification environment"

for command in systemd-detect-virt loginctl python3; do
  command -v "$command" >/dev/null || fail "missing dependency: $command"
done

[[ "${XDG_SESSION_TYPE:-}" == "wayland" ]] || fail "XDG_SESSION_TYPE must be wayland"

session_id="${SEMWRIGHT_EIS_SESSION_ID:-${XDG_SESSION_ID:-}}"
if [[ -z "$session_id" ]]; then
  session_id=$(loginctl show-user "$UID" -p Display --value)
fi
[[ -n "$session_id" ]] || fail "could not resolve logind session"

active=$(loginctl show-session "$session_id" -p Active --value)
session_type=$(loginctl show-session "$session_id" -p Type --value)
remote=$(loginctl show-session "$session_id" -p Remote --value)
class=$(loginctl show-session "$session_id" -p Class --value)
state=$(loginctl show-session "$session_id" -p State --value)
seat=$(loginctl show-session "$session_id" -p Seat --value)

[[ "$active" == "yes" ]] || fail "logind session is not active"
[[ "$session_type" == "wayland" ]] || fail "logind session type is not wayland"
[[ "$remote" == "no" ]] || fail "remote sessions are not accepted for local portal certification"
[[ "$class" == "user" ]] || fail "logind session class is not user"
[[ "$state" == "active" ]] || fail "logind session state is not active"

virt=$(systemd-detect-virt --vm 2>/dev/null || true)
if [[ -n "$virt" && "$virt" != "none" ]]; then
  isolation_mode="vm"
else
  [[ -n "$seat" && "$seat" != "seat0" ]] ||     fail "bare-metal seat0 is the owner-active boundary; use a VM or independent seat"
  isolation_mode="independent-seat"
  virt="none"
fi

desktop="${XDG_CURRENT_DESKTOP:-${XDG_SESSION_DESKTOP:-unknown}}"
expected="${SEMWRIGHT_EIS_EXPECT_DESKTOP:-}"
if [[ -n "$expected" ]]; then
  shopt -s nocasematch
  [[ "$desktop" == *"$expected"* ]] || fail "desktop '$desktop' does not match expected '$expected'"
  shopt -u nocasematch
fi

SEMWRIGHT_EIS_SESSION_ID_RESOLVED="$session_id" SEMWRIGHT_EIS_SEAT_RESOLVED="$seat" SEMWRIGHT_EIS_VIRT_RESOLVED="$virt" SEMWRIGHT_EIS_ISOLATION_MODE_RESOLVED="$isolation_mode" SEMWRIGHT_EIS_DESKTOP_RESOLVED="$desktop" python3 - <<'PY'
import json
import os

print(json.dumps({
    "status": "PASS_PREFLIGHT_ONLY",
    "certification_complete": False,
    "isolation_mode": os.environ["SEMWRIGHT_EIS_ISOLATION_MODE_RESOLVED"],
    "virtualization": os.environ["SEMWRIGHT_EIS_VIRT_RESOLVED"],
    "session_id": os.environ["SEMWRIGHT_EIS_SESSION_ID_RESOLVED"],
    "seat": os.environ["SEMWRIGHT_EIS_SEAT_RESOLVED"],
    "desktop": os.environ["SEMWRIGHT_EIS_DESKTOP_RESOLVED"],
    "next_step": (
        "Only inside this isolated authority boundary, use disposable fixtures and "
        "explicit human portal consent for keyboard target-delivery certification."
    ),
}, indent=2, sort_keys=True))
PY
