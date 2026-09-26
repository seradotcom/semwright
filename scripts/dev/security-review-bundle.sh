#!/usr/bin/env bash
set -euo pipefail

ROOT=$(git rev-parse --show-toplevel)
cd "$ROOT"

BASELINE_INPUT=${1:-HEAD}
BASELINE_SHA=$(git rev-parse "${BASELINE_INPUT}^{commit}")
SHORT_SHA=${BASELINE_SHA:0:12}
OUT=${2:-"/tmp/semwright-security-review-${SHORT_SHA}"}

required_paths=(
  "docs/security-review.md"
  "docs/requirements/SECURITY_THREAT_MODEL.md"
  "RELEASE_BLOCKERS.md"
  "VERIFY.md"
  "SECURITY.md"
  ".github/workflows/security.yml"
  "Cargo.lock"
  "deny.toml"
)

for path in "${required_paths[@]}"; do
  git cat-file -e "${BASELINE_SHA}:${path}" || {
    echo "security review bundle: missing ${path} at ${BASELINE_SHA}" >&2
    exit 2
  }
done

rm -rf "$OUT"
mkdir -p "$OUT/reference"

git archive --format=tar --prefix="semwright-${SHORT_SHA}/" "$BASELINE_SHA" | gzip -n -9 > "$OUT/semwright-source-${SHORT_SHA}.tar.gz"

for path in "${required_paths[@]}"; do
  dest="$OUT/reference/$path"
  mkdir -p "$(dirname "$dest")"
  git show "${BASELINE_SHA}:${path}" > "$dest"
done

REPO_URL=$(git config --get remote.origin.url || true)
SOURCE_ARCHIVE="semwright-source-${SHORT_SHA}.tar.gz"
SOURCE_SHA256=$(sha256sum "$OUT/$SOURCE_ARCHIVE" | awk '{print $1}')

cat > "$OUT/REVIEWER_REPORT_TEMPLATE.md" <<'EOF'
# Semwright independent security review report

Status: UNREVIEWED template. Completing this file is the responsibility of an independent reviewer.

## Review identity

- Reviewer / organization:
- Review start date:
- Review end date:
- Reviewed baseline SHA:
- Host / kernel / distribution:
- Rust and security-tool versions:
- Live desktop/application environments actually executed:

## Required review areas

Record evidence and findings for every area in reference/docs/security-review.md:

1. Authorization and confirmation
2. IPC and session identity
3. Object identity and focus
4. Portal authority
5. Filesystem confinement
6. Plugin/driver isolation
7. Federated MCP
8. Prompt-injection containment
9. Audit and disclosure
10. Resource and lifecycle safety
11. Supply chain
12. Platform boundaries

## Findings

| ID | Severity | Path / boundary | Reproduction | Impact | Remediation status |
|---|---|---|---|---|---|

## Executed evidence

List exact commands, fixtures, duration, tool versions and artifact hashes. A command that selected zero tests is not evidence.

## Residual risk / non-claims

Preserve the non-claims from reference/docs/security-review.md; do not upgrade them into guarantees.

## Independent conclusion

- Release-blocking findings still open:
- Non-blocking findings still open:
- Areas not executed:
- Reviewer conclusion:

R16 remains open until an independent reviewer completes a dated report tied to the exact baseline and the maintainer records any required remediation SHAs. This template and its generated bundle are not self-attestation.
EOF

BASELINE_SHA="$BASELINE_SHA" REPO_URL="$REPO_URL" SOURCE_ARCHIVE="$SOURCE_ARCHIVE" SOURCE_SHA256="$SOURCE_SHA256" python3 - "$OUT/manifest.json" <<'PY'
import json
import os
import sys

manifest = {
    "schema_version": 1,
    "status": "UNREVIEWED",
    "independent_review_required": True,
    "self_attestation": False,
    "baseline_sha": os.environ["BASELINE_SHA"],
    "repository_url": os.environ.get("REPO_URL", ""),
    "source_snapshot": {
        "archive": os.environ["SOURCE_ARCHIVE"],
        "sha256": os.environ["SOURCE_SHA256"],
        "method": "git archive of immutable commit",
    },
    "reference_files": [
        "reference/docs/security-review.md",
        "reference/docs/requirements/SECURITY_THREAT_MODEL.md",
        "reference/RELEASE_BLOCKERS.md",
        "reference/VERIFY.md",
        "reference/SECURITY.md",
        "reference/.github/workflows/security.yml",
        "reference/Cargo.lock",
        "reference/deny.toml",
    ],
    "review_report_template": "REVIEWER_REPORT_TEMPLATE.md",
    "closure_rule": "R16 may close only after an independent reviewer completes a dated report for this baseline and required remediation is recorded.",
}
with open(sys.argv[1], "w", encoding="utf-8") as f:
    json.dump(manifest, f, indent=2, sort_keys=True)
    f.write("\n")
PY

(
  cd "$OUT"
  sha256sum \
    "$SOURCE_ARCHIVE" \
    manifest.json \
    REVIEWER_REPORT_TEMPLATE.md \
    reference/docs/security-review.md \
    reference/docs/requirements/SECURITY_THREAT_MODEL.md \
    reference/RELEASE_BLOCKERS.md \
    reference/VERIFY.md \
    reference/SECURITY.md \
    reference/.github/workflows/security.yml \
    reference/Cargo.lock \
    reference/deny.toml \
    > SHA256SUMS
)

printf "%s\n" "$BASELINE_SHA" > "$OUT/BASELINE_SHA"
printf "security review bundle: %s\n" "$OUT"
printf "baseline: %s\n" "$BASELINE_SHA"
printf "source sha256: %s\n" "$SOURCE_SHA256"
printf "status: UNREVIEWED (independent review required)\n"
