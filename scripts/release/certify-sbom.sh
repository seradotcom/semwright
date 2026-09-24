#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
OUT=${1:-"$ROOT/dist/sbom"}
mkdir -p "$OUT"
rm -f "$OUT"/*.json "$OUT"/SHA256SUMS 2>/dev/null || true

if ! cargo cyclonedx --version | grep -q '0\.5\.9'; then
  echo "cargo-cyclonedx 0.5.9 is required" >&2
  exit 2
fi

export SOURCE_DATE_EPOCH=${SOURCE_DATE_EPOCH:-$(git -C "$ROOT" show -s --format=%ct HEAD)}
SOURCE_COMMIT=${SOURCE_COMMIT:-$(git -C "$ROOT" rev-parse HEAD)}
export SOURCE_COMMIT

generate_one() {
  local name=$1
  local manifest=$2
  local dir
  dir=$(dirname "$manifest")
  rm -f "$dir/$name.cdx" "$dir/$name.cdx.json" "$dir/$name.cdx.xml"
  cargo cyclonedx     --manifest-path "$manifest"     --format json     --spec-version 1.5     --override-filename "$name.cdx"
  mapfile -t outputs < <(find "$dir" -maxdepth 1 -type f -name "$name.cdx*" -print)
  if [ "${#outputs[@]}" -ne 1 ]; then
    printf 'Expected one SBOM for %s, got %s\n' "$name" "${outputs[*]-}" >&2
    exit 3
  fi
  cp "${outputs[0]}" "$OUT/$name.cdx.json"
  rm -f "${outputs[0]}"
  python3 "$ROOT/scripts/release/normalize_cyclonedx.py" \
    --path "$OUT/$name.cdx.json" --name "$name" --source-commit "$SOURCE_COMMIT"
}

cd "$ROOT"
generate_one semwright crates/cli/Cargo.toml
generate_one semwrightd crates/daemon/Cargo.toml
generate_one semwright-mcp crates/mcp/Cargo.toml
generate_one semwright-inspect crates/tui/Cargo.toml
generate_one semwright-sandbox crates/plugin-host/Cargo.toml

(
  cd "$OUT"
  sha256sum ./*.json | LC_ALL=C sort -k2 > SHA256SUMS
)

# Generate a second independent copy with the same SOURCE_DATE_EPOCH and prove byte identity.
SECOND=$(mktemp -d)
trap 'rm -rf "$SECOND"' EXIT
for item in semwright semwrightd semwright-mcp semwright-inspect semwright-sandbox; do
  manifest=
  case "$item" in
    semwright) manifest=crates/cli/Cargo.toml ;;
    semwrightd) manifest=crates/daemon/Cargo.toml ;;
    semwright-mcp) manifest=crates/mcp/Cargo.toml ;;
    semwright-inspect) manifest=crates/tui/Cargo.toml ;;
    semwright-sandbox) manifest=crates/plugin-host/Cargo.toml ;;
  esac
  dir=$(dirname "$manifest")
  rm -f "$dir/$item.cdx" "$dir/$item.cdx.json" "$dir/$item.cdx.xml"
  cargo cyclonedx     --manifest-path "$manifest"     --format json     --spec-version 1.5     --override-filename "$item.cdx"
  mapfile -t outputs < <(find "$dir" -maxdepth 1 -type f -name "$item.cdx*" -print)
  [ "${#outputs[@]}" -eq 1 ]
  mv "${outputs[0]}" "$SECOND/$item.cdx.json"
  python3 "$ROOT/scripts/release/normalize_cyclonedx.py" \
    --path "$SECOND/$item.cdx.json" --name "$item" --source-commit "$SOURCE_COMMIT"
done

for item in semwright semwrightd semwright-mcp semwright-inspect semwright-sandbox; do
  cmp "$OUT/$item.cdx.json" "$SECOND/$item.cdx.json"
done

echo "CycloneDX SBOM certification: PASS"
