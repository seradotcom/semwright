#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
ITERATIONS=${SEMWRIGHT_ATSPI_PRESSURE_ITERATIONS:-12}

cd "$ROOT"
mkdir -p verification/native-ci/atspi-pressure

for i in $(seq 1 "$ITERATIONS"); do
  echo "iteration=$i/$ITERATIONS"
  scripts/dev/atspi-live-ci.sh gtk
  cp verification/native-ci/atspi-gtk.log     "verification/native-ci/atspi-pressure/gtk-$(printf '%03d' "$i").log"
done

printf 'iterations=%s\nstatus=PASS\n' "$ITERATIONS"   > verification/native-ci/atspi-pressure/SUMMARY.txt
cat verification/native-ci/atspi-pressure/SUMMARY.txt
