#!/usr/bin/env bash
# Deterministic Agent Skills acceptance smoke against the fake Broker.
# No Skill scripts or live desktop operations are executed.
set -euo pipefail
cd "$(dirname "$0")/../.."
BIN_DIR="${BIN_DIR:-$PWD/target/debug}"
[[ -x "$BIN_DIR/semwrightd" && -x "$BIN_DIR/semwright" ]] || {
    echo 'Build semwrightd and semwright first.' >&2
    exit 2
}

WORK=$(mktemp -d)
BROKER_PID=''
cleanup() {
    if [[ -n "$BROKER_PID" ]]; then
        kill "$BROKER_PID" 2>/dev/null || true
        wait "$BROKER_PID" 2>/dev/null || true
    fi
    rm -rf -- "$WORK"
}
trap cleanup EXIT
chmod 700 "$WORK"
export XDG_RUNTIME_DIR="$WORK"
printf '[policy]\nprofile="desktop"\nallow=["workflow.record","workflow.manage"]\n' > "$WORK/policy.toml"
chmod 600 "$WORK/policy.toml"

"$BIN_DIR/semwrightd" --fake --config "$WORK/policy.toml" > "$WORK/daemon.log" 2>&1 &
BROKER_PID=$!
SOCKET="$WORK/semwright/fake.sock"
for _ in {1..100}; do
    [[ -S "$SOCKET" ]] && break
    kill -0 "$BROKER_PID" 2>/dev/null || {
        cat "$WORK/daemon.log" >&2
        exit 1
    }
    sleep 0.05
done
[[ -S "$SOCKET" ]] || { cat "$WORK/daemon.log" >&2; exit 1; }

"$BIN_DIR/semwright" --json skill validate skills/semwright-core
"$BIN_DIR/semwright" --json skill inspect skills/semwright-core
"$BIN_DIR/semwright" --socket "$SOCKET" --json skill doctor skills/semwright-core
"$BIN_DIR/semwright" --socket "$SOCKET" --json skill doctor skills/semwright-workflow-distillation
"$BIN_DIR/semwright" --socket "$SOCKET" --json skill test skills/semwright-workflow-distillation

# Preserve Agent Skills name/directory equality for the copied package.
cp -R skills/semwright-core "$WORK/semwright-core"
"$BIN_DIR/semwright" --socket "$SOCKET" --json skill lock "$WORK/semwright-core"
"$BIN_DIR/semwright" --socket "$SOCKET" --json skill doctor "$WORK/semwright-core"
"$BIN_DIR/semwright" --json skill bundle "$WORK/semwright-core" "$WORK/semwright-core.zip"
test -s "$WORK/semwright-core.zip"

"$BIN_DIR/semwright" --json skill scaffold scaffolded-skill "$WORK/scaffolded-skill"
"$BIN_DIR/semwright" --json skill validate "$WORK/scaffolded-skill"

"$BIN_DIR/semwright" --socket "$SOCKET" --json skill export capabilities.describe     --output "$WORK/exported-capability"
"$BIN_DIR/semwright" --json skill validate "$WORK/exported-capability"
