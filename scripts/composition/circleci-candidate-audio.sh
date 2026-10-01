#!/usr/bin/env bash
set -euo pipefail
test "${CIRCLECI:-}" = true
test "${CIRCLE_BRANCH:-}" = "integration/composition-av"
test "${SEMWRIGHT_CIRCLECI_CANDIDATE_CERTIFICATION:-}" = true
test "$(git rev-parse HEAD)" = "${CIRCLE_SHA1}"
source "$HOME/.cargo/env"
root="$PWD"
evidence="$root/verification/circleci-certification/audio"
mkdir -p "$evidence" "$root/verification/audio"
sudo apt-get update
sudo apt-get install -y \
  faust=2.70.3+ds-1.1build2 libsndfile1-dev libebur128-dev=1.2.6-1build1 \
  'ardour=1:8.4.0+ds1-2ubuntu8' 'ardour-data=1:8.4.0+ds1-2ubuntu8' \
  bubblewrap apparmor-profiles apparmor-utils libpipewire-0.3-dev strace
dpkg-query -W faust libsndfile1-dev libebur128-dev ardour ardour-data \
  > "$evidence/runtime-packages.txt"
bash scripts/dev/ci-driver-bwrap-profile.sh "$evidence/sandbox-setup.log"
python3 scripts/audio/run-suite.py portable
cargo fmt -p semwright-audio-authoring -p semwright-audio-domain \
  -p semwright-faust-audio-driver -p semwright-ardour-audio-driver -- --check
cargo clippy --locked -p semwright-audio-authoring -p semwright-audio-domain \
  -p semwright-faust-audio-driver -p semwright-ardour-audio-driver \
  --all-targets -- -D warnings
helper_dir="$(mktemp -d)"
cleanup(){ rm -rf "$helper_dir"; }
trap cleanup EXIT
g++ -std=c++17 -O2 -Wall -Wextra -Werror \
  integrations/audio/faust-runtime/main.cpp -lfaust -lsndfile \
  -o "$helper_dir/semwright-faust-interpreter"
g++ -std=c++17 -O2 -Wall -Wextra -Werror \
  integrations/audio/analysis-runtime/main.cpp -lebur128 -lsndfile \
  -o "$helper_dir/semwright-audio-meter"
cargo build --locked -p semwright-plugin-host -p semwright-cli \
  -p semwright-faust-audio-driver -p semwright-ardour-audio-driver --bins
export SEMWRIGHT_TEST_FAUST_VERSION=2.70.3
export SEMWRIGHT_TEST_FAUST_HELPER="$helper_dir/semwright-faust-interpreter"
export SEMWRIGHT_TEST_AUDIO_METER="$helper_dir/semwright-audio-meter"
export SEMWRIGHT_TEST_FAUST_LIBRARIES=/usr/share/faust
export SEMWRIGHT_TEST_SANDBOX_HELPER="$root/target/debug/semwright-sandbox"
scripts/audio/faust-confinement-diagnostics.sh \
  "$SEMWRIGHT_TEST_FAUST_HELPER" "$SEMWRIGHT_TEST_SANDBOX_HELPER" \
  /usr/share/faust "$evidence/faust-confinement"
python3 scripts/audio/run-suite.py faust
python3 scripts/audio/run-suite.py faust-host
python3 scripts/audio/run-suite.py analysis-host
LUA="$(find /usr/lib -type f -path '*ardour8*' -name luasession -perm /111 -print -quit)"
CREATE="$(find /usr/lib -type f -path '*ardour8*' -name ardour8-new_session -perm /111 -print -quit)"
EXPORT="$(find /usr/lib -type f -path '*ardour8*' -name ardour8-export -perm /111 -print -quit)"
test -n "$LUA" && test -n "$CREATE" && test -n "$EXPORT"
sha256sum "$LUA" "$CREATE" "$EXPORT" > "$evidence/ardour-tools.sha256"
export SEMWRIGHT_TEST_ARDOUR_LUA="$LUA"
export SEMWRIGHT_TEST_ARDOUR_CREATE="$CREATE"
export SEMWRIGHT_TEST_ARDOUR_EXPORT="$EXPORT"
direct_home="$(mktemp -d)"
direct_session="$(mktemp -d)"
rm -rf "$direct_session"; mkdir -p "$direct_home"
env -i HOME="$direct_home" LANG=C.UTF-8 LC_ALL=C.UTF-8 PATH=/usr/bin:/bin \
  LD_LIBRARY_PATH=/usr/lib/ardour8 ARDOUR_DATA_PATH=/usr/share/ardour8 \
  ARDOUR_CONFIG_PATH=/etc/ardour8 ARDOUR_DLL_PATH=/usr/lib/ardour8 \
  VAMP_PATH=/usr/lib/ardour8/vamp "$CREATE" -s 48000 "$direct_session" Direct \
  > "$evidence/ardour-direct-create.stdout" 2> "$evidence/ardour-direct-create.stderr"
test -s "$direct_session/Direct.ardour"
rm -rf "$direct_home" "$direct_session"
cargo test --locked -p semwright-ardour-audio-driver \
  runtime::tests::persisted_plugin_automation_native_diagnostic \
  -- --ignored --exact --nocapture
python3 scripts/audio/run-suite.py ardour-host
python3 scripts/audio/package-development.py \
  --semwright "$root/target/debug/semwright" \
  --faust-driver "$root/target/debug/semwright-faust-audio-driver" \
  --analysis-driver "$root/target/debug/semwright-audio-analysis-driver" \
  --ardour-driver "$root/target/debug/semwright-ardour-audio-driver" \
  --faust-helper "$SEMWRIGHT_TEST_FAUST_HELPER" \
  --audio-meter "$SEMWRIGHT_TEST_AUDIO_METER" \
  --ardour-lua "$LUA" --ardour-create "$CREATE" --ardour-export "$EXPORT" \
  --faust-libraries /usr/share/faust --output "$evidence/packages"
python3 - <<'PY'
import json, os
from pathlib import Path
root=Path("verification/audio")
suites=["portable","faust","faust-host","analysis-host","ardour-host"]
receipts={name:json.loads((root/f"{name}.json").read_text()) for name in suites}
if any(r.get("status")!="PASS" or not r.get("certification_eligible") for r in receipts.values()):
    raise SystemExit("audio candidate suite receipt missing PASS/certification authority")
out=Path("verification/circleci-certification/audio/final-certification.json")
out.write_text(json.dumps({"schema_version":1,"status":"PASS",
 "candidate_sha":os.environ["CIRCLE_SHA1"],"provider":"circleci",
 "workflow_id":os.environ["CIRCLE_WORKFLOW_ID"],"job":os.environ["CIRCLE_JOB"],
 "suites":{k:{"passed":v["passed"],"expected":v["expected_tests"]} for k,v in receipts.items()}},
 indent=2,sort_keys=True)+"\n")
PY
