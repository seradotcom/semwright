# Remediation log

Affected initial SHA: `241000c268d1bf1dc29d4e91a913097ac0d020cb`. Audit fix commit: `53c5344ec3fea500b7cade8471928a25bb194048`.
No fix has a recorded main merge SHA yet; branch implementation is not merged-main verification.

### PRE-001 — destructive-release-tool

Severity: **MEDIUM**. Status: **FIX_COMMITTED_CI_PENDING**.

Review-bundle generation deleted a caller-selected existing output path.

Source: `scripts/dev/security-review-bundle.sh`.

Fix commit: `53c5344ec3fea500b7cade8471928a25bb194048`. Main merge: **not established**.

Verification/regression: tests/python/test_security_review_bundle_safety.py; 12 executed tests passed.

### PRE-002 — bundle-integrity

Severity: **LOW**. Status: **FIX_COMMITTED_CI_PENDING**.

BASELINE_SHA was outside SHA256SUMS.

Source: `scripts/dev/security-review-bundle.sh`.

Fix commit: `53c5344ec3fea500b7cade8471928a25bb194048`. Main merge: **not established**.

Verification/regression: baseline tampering regression passed.

### PRE-003 — terminal-display

Severity: **MEDIUM**. Status: **FIX_COMMITTED_CI_PENDING**.

CLI human errors could contain raw terminal controls from filenames; JSON output left C1/format controls literal.

Source: `crates/cli/src/lib.rs; crates/cli/src/semwright.rs`.

Fix commit: `53c5344ec3fea500b7cade8471928a25bb194048`. Main merge: **not established**.

Verification/regression: Rust unit and production CLI tests added; execution delegated to Actions.

### PRE-004 — claim-evidence-drift

Severity: **MEDIUM**. Status: **REMEDIATION_IN_AUDIT_BRANCH**.

README/VERIFY/compatibility and doctor contradicted implemented/tested paths and overstated exact-commit CI. RELEASE_BLOCKERS.md also needs owner-coordinated reconciliation.

Source: `README.md; VERIFY.md; docs/compatibility.md; CHANGELOG.md; RELEASE_BLOCKERS.md; crates/core/src/lib.rs`.

Fix commit: `53c5344ec3fea500b7cade8471928a25bb194048`. Main merge: **not established**.

Verification/regression: doctor contract regression added; source/doc checks required.

### PRE-005 — exact-sha-ci-failure

Severity: **GATE**. Status: **OPEN_OWNER_WINDOWS**.

Observed main Windows ARM64 native UIA test fails StaleReference; later job steps were skipped.

Source: `Actions run 36394993424 job 108839165329`.

Fix commit: `NONE / owner work pending`. Main merge: **not established**.

Verification/regression: owner must address fixture lifecycle/root cause and provide new exact-SHA green evidence.

### PRE-006 — false-positive-test-evidence

Severity: **MEDIUM**. Status: **OPEN_OWNER_WINDOWS**.

PR153 marks interactive rows PASS from cargo exit code alone; zero-selected tests and generic runner routing are not rejected.

Source: `PR153 at ab5b4f95d2193a7e1070fc5158c037c0b8f3a281`.

Fix commit: `NONE / owner work pending`. Main merge: **not established**.

Verification/regression: positive executed count, ignored-only/zero-test rejection and dedicated runner label required.

### PRE-007 — unreconciled-unique-work

Severity: **GATE**. Status: **OPEN_BEHAVIORAL_REVIEW**.

48 unmerged distinct heads still require behavioral disposition; security-relevant dirty worktrees are preserved, not declared obsolete.

Source: `inventory/branch-tree-comparison.json; inventory/worktrees.json`.

Fix commit: `NONE / owner work pending`. Main merge: **not established**.

Verification/regression: compare high-priority dirty diffs, missing changes and regression evidence against main.

### PRE-008 — active-runtime-remediation

Severity: **GATE**. Status: **OPEN_OWNER_BLENDER**.

PR154 introduces owner-pinned Blender runtime alongside new export surface and currently failing checks; freeze must resolve the necessary security/runtime subset.

Source: `PR154 at d46b241b97beb14093da71582c5106e6d616271c`.

Fix commit: `NONE / owner work pending`. Main merge: **not established**.

Verification/regression: owner root-cause fixes and exact-head plus merged-main checks.

## Scope

The bundle safety tests execute exclusively against disposable repositories. No public secret values
were printed or added to findings. No test was skipped, weakened, converted to flaky, or replaced by
an unconditional PASS. Independent review and release admission remain unchanged.
