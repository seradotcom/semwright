#!/usr/bin/env bash
set -euo pipefail

: "${SEMWRIGHT_AUTHORING_EVIDENCE:?SEMWRIGHT_AUTHORING_EVIDENCE is required}"
out="$SEMWRIGHT_AUTHORING_EVIDENCE"
mkdir -p "$out"

cargo build --locked -p semwright-cli --bin semwright

target/debug/semwright --json skill validate skills/semwright-blender-production   | tee "$out/skill-validate.json"

target/debug/semwright --json skill bundle   skills/semwright-blender-production   "$out/semwright-blender-production.zip"   | tee "$out/skill-bundle.json"

# Rebuild once into an independent path so the skill-area iteration itself proves
# deterministic bundle bytes instead of deferring that discovery to certification.
target/debug/semwright --json skill bundle   skills/semwright-blender-production   /tmp/semwright-blender-production-second.zip   >/tmp/semwright-blender-production-second.json
cmp "$out/semwright-blender-production.zip" /tmp/semwright-blender-production-second.zip
rm -f /tmp/semwright-blender-production-second.zip   /tmp/semwright-blender-production-second.json

test -s "$out/skill-validate.json"
test -s "$out/skill-bundle.json"
test -s "$out/semwright-blender-production.zip"

(
  cd "$out"
  sha256sum semwright-blender-production.zip     | tee semwright-blender-production.zip.sha256
  sha256sum --check --strict semwright-blender-production.zip.sha256
)
