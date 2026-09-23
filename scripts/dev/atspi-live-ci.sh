#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
PHASE=${1:-}
case "$PHASE" in
  gtk|qt) ;;
  *) echo "usage: $0 <gtk|qt>" >&2; exit 2 ;;
esac

if [[ ${SEMWRIGHT_ATSPI_INNER:-0} != 1 ]]; then
  runtime=${XDG_RUNTIME_DIR:-}
  [[ -n "$runtime" ]]
  mkdir -p "$runtime"
  chmod 700 "$runtime"
  exec xvfb-run -a -s "-screen 0 1280x720x24 -nolisten tcp" \
    env SEMWRIGHT_ATSPI_INNER=1 dbus-run-session -- "$0" "$PHASE"
fi

cd "$ROOT"
mkdir -p verification/native-ci

if [[ "$PHASE" == gtk ]]; then
  gdbus call --session --dest org.a11y.Bus --object-path /org/a11y/bus \
    --method org.freedesktop.DBus.Properties.Set org.a11y.Status IsEnabled "<true>" >/dev/null
  gdbus call --session --dest org.a11y.Bus --object-path /org/a11y/bus \
    --method org.freedesktop.DBus.Properties.Set org.a11y.Status ScreenReaderEnabled "<true>" >/dev/null
  raw_address=$(gdbus call --session --dest org.a11y.Bus --object-path /org/a11y/bus \
    --method org.a11y.Bus.GetAddress)
  AT_SPI_BUS_ADDRESS=${raw_address:2:${#raw_address}-5}
  [[ "$AT_SPI_BUS_ADDRESS" == unix:* ]]
  export AT_SPI_BUS_ADDRESS

  printf "%s_AT_SPI_BUS_ADDRESS=%s\n" "$PHASE" "$AT_SPI_BUS_ADDRESS" \
    | sed -E "s/guid=[^, ]+/guid=<redacted>/" \
    | tee -a verification/native-ci/atspi-address.log

  registry_ready=0
  for _ in $(seq 1 100); do
    if timeout 2s gdbus call --address "$AT_SPI_BUS_ADDRESS" \
      --dest org.a11y.atspi.Registry \
      --object-path /org/a11y/atspi/registry \
      --method org.a11y.atspi.Registry.GetRegisteredEvents \
      > "verification/native-ci/atspi-${PHASE}-registry.log" 2>&1; then
      registry_ready=1
      break
    fi
    sleep 0.1
  done
  cat "verification/native-ci/atspi-${PHASE}-registry.log"
  test "$registry_ready" -eq 1
  echo "phase=${PHASE}_registry_ready" | tee -a verification/native-ci/atspi-phases.log
else
  # Qt 6.11 rechecks accessibility on the next event-loop turn even when
  # AT_SPI_BUS_ADDRESS is already present. Resolve the isolated bus before the
  # fixture starts so Qt and Semwright cannot race service activation.
  gdbus call --session --dest org.a11y.Bus --object-path /org/a11y/bus     --method org.freedesktop.DBus.Properties.Set org.a11y.Status IsEnabled "<true>" >/dev/null
  gdbus call --session --dest org.a11y.Bus --object-path /org/a11y/bus     --method org.freedesktop.DBus.Properties.Set org.a11y.Status ScreenReaderEnabled "<true>" >/dev/null
  raw_address=$(gdbus call --session --dest org.a11y.Bus --object-path /org/a11y/bus     --method org.a11y.Bus.GetAddress)
  AT_SPI_BUS_ADDRESS=${raw_address:2:${#raw_address}-5}
  [[ "$AT_SPI_BUS_ADDRESS" == unix:* ]]
  export AT_SPI_BUS_ADDRESS

  printf "%s_AT_SPI_BUS_ADDRESS=%s\n" "$PHASE" "$AT_SPI_BUS_ADDRESS"     | sed -E "s/guid=[^, ]+/guid=<redacted>/"     | tee -a verification/native-ci/atspi-address.log

  registry_ready=0
  for _ in $(seq 1 100); do
    if timeout 2s gdbus call --address "$AT_SPI_BUS_ADDRESS"       --dest org.a11y.atspi.Registry       --object-path /org/a11y/atspi/registry       --method org.a11y.atspi.Registry.GetRegisteredEvents       > "verification/native-ci/atspi-${PHASE}-registry.log" 2>&1; then
      registry_ready=1
      break
    fi
    sleep 0.1
  done
  cat "verification/native-ci/atspi-${PHASE}-registry.log"
  test "$registry_ready" -eq 1
  echo "phase=${PHASE}_registry_ready" | tee -a verification/native-ci/atspi-phases.log
fi
case "$PHASE" in
  gtk)
    test_name=live_atspi_gtk_delta_resync_and_stale_refs
    ;;
  qt)
    : "${SEMWRIGHT_TEST_QT_FIXTURE:?SEMWRIGHT_TEST_QT_FIXTURE is required for qt}"
    export SEMWRIGHT_TEST_QT_PLATFORM=${SEMWRIGHT_TEST_QT_PLATFORM:-xcb}
    test_name=live_atspi_qt_delta_resync_and_stale_refs
    ;;
esac

echo "phase=${PHASE}_start" | tee -a verification/native-ci/atspi-phases.log
rc=0
timeout --signal=TERM --kill-after=5s 90s \
  cargo test --locked -p semwright-platform-linux "$test_name" \
    -- --ignored --nocapture --test-threads=1 \
  > "verification/native-ci/atspi-${PHASE}.log" 2>&1 || rc=$?
cat "verification/native-ci/atspi-${PHASE}.log"
if [[ "$rc" -ne 0 ]]; then
  {
    echo "phase=${PHASE}_failed rc=$rc"
    echo "--- bounded AT-SPI process diagnostics ---"
    ps -ef | grep -E '[a]t-spi|[d]bus-daemon|[X]vfb|[a]tspi-qt-fixture' || true
    echo "--- registry applications ---"
    timeout 2s gdbus call --address "$AT_SPI_BUS_ADDRESS"       --dest org.a11y.atspi.Registry       --object-path /org/a11y/atspi/accessible/root       --method org.a11y.atspi.Accessible.GetChildren || true
  } | tee -a "verification/native-ci/atspi-${PHASE}-diagnostics.log"
  echo "phase=${PHASE}_failed rc=$rc" | tee -a verification/native-ci/atspi-phases.log
  exit "$rc"
fi
echo "phase=${PHASE}_done" | tee -a verification/native-ci/atspi-phases.log
