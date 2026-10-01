#!/usr/bin/env bash
set -euo pipefail

if [[ "${CIRCLECI:-}" != "true" || "${CIRCLE_BRANCH:-}" != "integration/composition-av" ]]; then
  echo "Combined AV execution requires CircleCI integration/composition-av." >&2
  exit 1
fi

root="$PWD"
evidence="verification/circleci-composition/combined-av"
mkdir -p "$evidence"

sudo apt-get update
sudo apt-get install -y   faust=2.70.3+ds-1.1build2   libsndfile1-dev   libebur128-dev=1.2.6-1build1   bubblewrap apparmor-profiles apparmor-utils   melt ffmpeg libpipewire-0.3-dev

faust_stage="$(mktemp -d)"
helper_dir="$(mktemp -d)"
cleanup() {
  chmod -R u+w "$faust_stage" 2>/dev/null || true
  rm -rf "$faust_stage" "$helper_dir"
}
trap cleanup EXIT
export faust_stage

python3 - <<'PY'
import hashlib, json, os, shutil
from pathlib import Path
src=Path('/usr/share/faust')
dst=Path(os.environ['faust_stage'])
pins={}
for source in sorted(src.rglob('*.lib')):
    if not source.is_file():
        continue
    relative=source.relative_to(src)
    target=dst/relative
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source,target,follow_symlinks=True)
    target.chmod(0o400)
    pins[relative.as_posix()]=hashlib.sha256(target.read_bytes()).hexdigest()
if 'stdfaust.lib' not in pins or len(pins)<2:
    raise SystemExit('Faust library inventory is incomplete')
runtime=dst/'semwright-runtime.json'
runtime.write_text(json.dumps({'schema_version':1,'compiler_version':'2.70.3','libraries':pins},separators=(',',':'),sort_keys=True))
runtime.chmod(0o400)
for directory in sorted((p for p in dst.rglob('*') if p.is_dir()), reverse=True):
    directory.chmod(0o500)
dst.chmod(0o500)
if any(path.is_symlink() for path in dst.rglob('*')):
    raise SystemExit('staged Faust tree contains symlinks')
print(f'staged {len(pins)} Faust libraries')
PY

g++ -std=c++17 -O2 -Wall -Wextra -Werror   integrations/audio/faust-runtime/main.cpp   -lfaust -lsndfile -o "$helper_dir/semwright-faust-interpreter"
g++ -std=c++17 -O2 -Wall -Wextra -Werror   integrations/audio/analysis-runtime/main.cpp   -lebur128 -lsndfile -o "$helper_dir/semwright-audio-meter"

cargo build --locked   -p semwright-plugin-host --bin semwright-sandbox   -p semwright-driver-motion-canvas --bin semwright-motion-canvas-driver   -p semwright-faust-audio-driver --bins   -p semwright-mlt-video-driver --bin semwright-mlt-video-driver

export SEMWRIGHT_TEST_COMBINED_AV=1
export SEMWRIGHT_TEST_COMBINED_MOTION_DRIVER="$root/target/debug/semwright-motion-canvas-driver"
export SEMWRIGHT_TEST_COMBINED_FAUST_DRIVER="$root/target/debug/semwright-faust-audio-driver"
export SEMWRIGHT_TEST_COMBINED_ANALYSIS_DRIVER="$root/target/debug/semwright-audio-analysis-driver"
export SEMWRIGHT_TEST_COMBINED_MLT_DRIVER="$root/target/debug/semwright-mlt-video-driver"
export SEMWRIGHT_TEST_SANDBOX_HELPER="$root/target/debug/semwright-sandbox"
export SEMWRIGHT_TEST_MOTION_RUNTIME="$root/integrations/motion-canvas/runtime"
export SEMWRIGHT_TEST_FAUST_VERSION=2.70.3
export SEMWRIGHT_TEST_FAUST_HELPER="$helper_dir/semwright-faust-interpreter"
export SEMWRIGHT_TEST_AUDIO_METER="$helper_dir/semwright-audio-meter"
export SEMWRIGHT_TEST_FAUST_LIBRARIES="$faust_stage"
export SEMWRIGHT_TEST_MELT=/usr/bin/melt
export SEMWRIGHT_TEST_FFPROBE=/usr/bin/ffprobe
export SEMWRIGHT_TEST_FFMPEG=/usr/bin/ffmpeg
export SEMWRIGHT_TEST_BWRAP=/usr/bin/bwrap

cargo test --locked -p semwright-av-composition   --test combined_native   combined_a_b_native_av_candidate_uses_post_encode_audio_and_full_scan_sync   -- --ignored --exact --nocapture   2>&1 | tee "$evidence/combined-native.log"

python3 - <<'PY'
import json, os
from pathlib import Path
path=Path('verification/circleci-composition/combined-av/iteration-classification.json')
cert=os.environ.get('SEMWRIGHT_CIRCLECI_CANDIDATE_CERTIFICATION')=='true'
path.write_text(json.dumps({
    'schema_version':1,
    'tested_sha':os.environ['CIRCLE_SHA1'],
    'ci_provider':'circleci',
    'classification':'CANDIDATE_CERTIFICATION' if cert else 'PRIVATE_ITERATION_DIAGNOSTIC',
    'certification_eligible':cert,
    'scope':'combined-native-av-e2e',
},indent=2)+"\n")
PY
