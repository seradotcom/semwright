#!/usr/bin/env bash
set -euo pipefail
[[ "${GITHUB_ACTIONS:-}" == true ]] || { echo 'GitHub-hosted CI only'; exit 1; }
[[ "$(git rev-parse HEAD)" == "$EXPECTED_SHA" ]] || { echo 'source SHA mismatch'; exit 1; }
case "$1" in godot|blender|native-all) suite="$1";; *) echo 'unknown native selector'; exit 1;; esac
mkdir -p verification/effects
sudo apt-get update -qq
sudo apt-get install -y bubblewrap fontconfig xz-utils libxrender1 libxi6 libxfixes3 libxkbcommon0 libsm6
bash scripts/dev/ci-driver-bwrap-profile.sh verification/effects/sandbox-setup.log
sudo mkdir -p /opt/semwright-effect-runtimes
sudo chown "$(id -u):$(id -g)" /opt/semwright-effect-runtimes
runtime=/opt/semwright-effect-runtimes
if [[ "$suite" == godot || "$suite" == native-all ]]; then
  curl --fail --location --retry 3 \
    'https://github.com/godotengine/godot-builds/releases/download/4.7.2-stable/Godot_v4.7.2-stable_linux.x86_64.zip' \
    -o "$RUNNER_TEMP/effects-godot.zip"
  unzip -q "$RUNNER_TEMP/effects-godot.zip" -d "$runtime"
  export GODOT_BIN="$runtime/Godot_v4.7.2-stable_linux.x86_64"
  echo "cadd3204e728a35d3f13adb7fd0d7902636b79f6b95c40c265eb73b6c35329e4  $GODOT_BIN" | sha256sum --check -
  "$GODOT_BIN" --version > verification/effects/godot-version.txt
  sha256sum "$GODOT_BIN" > verification/effects/godot-binary.sha256
  cargo run --locked -p semwright-effect-conformance --example native -- godot
fi
if [[ "$suite" == blender || "$suite" == native-all ]]; then
  curl --fail --location --retry 3 \
    'https://download.blender.org/release/Blender4.5/blender-4.5.14-linux-x64.tar.xz' \
    -o "$RUNNER_TEMP/effects-blender.tar.xz"
  echo "9ba871ff2ecd36526b77432745980b7e6664ecd0c7ca11c48849073dcfe06da3  $RUNNER_TEMP/effects-blender.tar.xz" | sha256sum --check -
  tar -xJf "$RUNNER_TEMP/effects-blender.tar.xz" -C "$runtime"
  export BLENDER_BIN="$runtime/blender-4.5.14-linux-x64/blender"
  "$BLENDER_BIN" --version > verification/effects/blender-version.txt
  grep -Fx 'Blender 4.5.14 LTS' verification/effects/blender-version.txt
  sha256sum "$BLENDER_BIN" > verification/effects/blender-binary.sha256
  cargo run --locked -p semwright-effect-conformance --example native -- blender
fi
