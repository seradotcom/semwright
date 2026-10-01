#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 3 ]]; then
  echo "usage: $0 <ref> <source_sha> <base_sha>" >&2
  exit 64
fi

ref=$1
source_sha=$2
base_sha=$3

[[ "$source_sha" =~ ^[0-9a-f]{40}$ ]] || { echo "source_sha must be a full lowercase SHA" >&2; exit 64; }
[[ "$base_sha" =~ ^[0-9a-f]{40}$ ]] || { echo "base_sha must be a full lowercase SHA" >&2; exit 64; }

resolved=$(git rev-parse "$ref^{commit}")
if [[ "$resolved" != "$source_sha" ]]; then
  echo "ref $ref resolves to $resolved, not requested source_sha $source_sha" >&2
  exit 65
fi
git cat-file -e "$base_sha^{commit}"
git merge-base --is-ancestor "$base_sha" "$source_sha" || {
  echo "base_sha is not an ancestor of source_sha" >&2
  exit 65
}

gh workflow run candidate-certification.yml \
  --ref "$ref" \
  -f source_sha="$source_sha" \
  -f base_sha="$base_sha"

echo "dispatched exact candidate certification for $source_sha against $base_sha on ref $ref"
