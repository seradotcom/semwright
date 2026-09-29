#!/usr/bin/env bash
set -euo pipefail
TIDELING_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
TIDELING_GODOT="${GODOT_BIN:-$HOME/.cache/semwright-tools/godot-4.7.2/godot}"
if [[ ! -x "$TIDELING_GODOT" ]]; then
  TIDELING_GODOT="$(command -v godot || command -v godot4 || true)"
fi
if [[ -z "$TIDELING_GODOT" || ! -x "$TIDELING_GODOT" ]]; then
  echo 'Set GODOT_BIN to an installed Godot 4 executable.' >&2
  exit 1
fi
exec "$TIDELING_GODOT" --path "$TIDELING_ROOT/project" "$@"
