#!/usr/bin/env bash
# Only deterministic fixtures. No user desktop connection or persistent service.
set -euo pipefail
cd "$(dirname "$0")/../.."
BIN_DIR="${BIN_DIR:-$PWD/target/debug}"
[[ -x "$BIN_DIR/semwrightd" && -x "$BIN_DIR/computerctl" ]] || { echo 'Build the workspace first.' >&2; exit 2; }
WORK=$(mktemp -d)
BROKER_PID=''
cleanup() {
    if [[ -n "$BROKER_PID" ]]; then kill "$BROKER_PID" 2>/dev/null || true; wait "$BROKER_PID" 2>/dev/null || true; fi
    rm -rf -- "$WORK"
}
trap cleanup EXIT
chmod 700 "$WORK"
export XDG_RUNTIME_DIR="$WORK"
printf '[policy]\nprofile="desktop"\n' > "$WORK/policy.toml"
chmod 600 "$WORK/policy.toml"
"$BIN_DIR/semwrightd" --fake --config "$WORK/policy.toml" > "$WORK/daemon.log" 2>&1 &
BROKER_PID=$!
SOCKET="$WORK/semwright/fake.sock"
for _ in {1..100}; do
    [[ -S "$SOCKET" ]] && break
    kill -0 "$BROKER_PID" 2>/dev/null || { cat "$WORK/daemon.log" >&2; exit 1; }
    sleep 0.05
done
[[ -S "$SOCKET" ]] || { cat "$WORK/daemon.log" >&2; exit 1; }
"$BIN_DIR/computerctl" --socket "$SOCKET" --json doctor
"$BIN_DIR/computerctl" --socket "$SOCKET" --json ui find --name Save
"$BIN_DIR/computerctl" --socket "$SOCKET" --json recipe run recipes/fake-export.yaml
"$BIN_DIR/computerctl" --socket "$SOCKET" --json audit tail
