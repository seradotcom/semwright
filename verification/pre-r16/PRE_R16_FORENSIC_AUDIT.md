# Pre-R16 forensic audit

**Disposition: NOT_READY_FOR_INDEPENDENT_R16_REVIEW. R16: OPEN.**

Audit start: `241000c268d1bf1dc29d4e91a913097ac0d020cb`. Remediation commit: `53c5344ec3fea500b7cade8471928a25bb194048`.
This record captures completed reconstruction and focused fixes, plus explicitly unfinished review.
It does not claim the entire repository or all historical branches have been fully audited.

## What was inspected

The initial origin/main snapshot, canonical Git status, all then-visible branch refs and worktrees,
154 historical PR records, three subsequently observed open PRs, all 12 initial-SHA workflow results
and job/step records, release/security documentation, source markers, tracked binary/secret patterns,
18 workflow definitions, version/release metadata and the official security-review bundle script.
Static security review covered policy, broker dispatch/ref/focus enforcement, IPC/session lifecycle,
audit failure semantics, jobs, workflow recording/promotion, Linux filesystem confinement,
artifact handoff, federation launch and Linux sandbox construction. The exact coverage limits appear
in SECURITY_PRECHECK.md; broad marker inventories are not proof that every match was reviewed.

## What was found

| Finding | Severity | Status | Summary |
|---|---|---|---|
| PRE-001 | MEDIUM | FIX_COMMITTED_CI_PENDING | Review-bundle generation deleted a caller-selected existing output path. |
| PRE-002 | LOW | FIX_COMMITTED_CI_PENDING | BASELINE_SHA was outside SHA256SUMS. |
| PRE-003 | MEDIUM | FIX_COMMITTED_CI_PENDING | CLI human errors could contain raw terminal controls from filenames; JSON output left C1/format controls literal. |
| PRE-004 | MEDIUM | REMEDIATION_IN_AUDIT_BRANCH | README/VERIFY/compatibility and doctor contradicted implemented/tested paths and overstated exact-commit CI. RELEASE_BLOCKERS.md also needs owner-coordinated reconciliation. |
| PRE-005 | GATE | OPEN_OWNER_WINDOWS | Observed main Windows ARM64 native UIA test fails StaleReference; later job steps were skipped. |
| PRE-006 | MEDIUM | OPEN_OWNER_WINDOWS | PR153 marks interactive rows PASS from cargo exit code alone; zero-selected tests and generic runner routing are not rejected. |
| PRE-007 | GATE | OPEN_BEHAVIORAL_REVIEW | 48 unmerged distinct heads still require behavioral disposition; security-relevant dirty worktrees are preserved, not declared obsolete. |
| PRE-008 | GATE | OPEN_OWNER_BLENDER | PR154 introduces owner-pinned Blender runtime alongside new export surface and currently failing checks; freeze must resolve the necessary security/runtime subset. |

`GATE` denotes an evidence/reconciliation blocker, not a CVSS severity or demonstrated vulnerability.
No independent severity assessment is implied. The eight records include four audit-owned changes,
one checksum subfinding, owner work and unresolved archaeology; they are not eight exploited bugs.

## What was fixed

The official bundle generator no longer removes or replaces a supplied output path. It creates
private exclusive output, rejects existing files/directories/symlinks and protected destinations,
and includes BASELINE_SHA in its checksum list. Twelve disposable-repository regressions passed.
CLI diagnostics now escape terminal controls in untrusted human fields and JSON rendering escapes
C1/format controls without changing parsed values. Rust unit and production-binary regressions are
added, but are not counted as passed until Actions executes them. The obsolete PipeWire-unimplemented
doctor field is removed with a broker regression. Documentation is being reconciled in this branch.

## What remains

Initial-main Windows ARM64 is red. Owner PR153 still has evidence-admission and runner-routing gaps;
PR154 mixes relevant runtime isolation work with a new GLB capability and failing checks. The audit
must not freeze around either known issue. Forty-eight unmerged heads require behavioral disposition;
security-relevant dirty worktrees cannot be called redundant from reachability or patch identity alone.
No whole-runtime soak, physical hardware campaign, full-history secret scan or independent review was
performed. The audit branch's new Rust tests and final exact-SHA gates still require execution.

## Deliberately deferred

TIDELING/demo expansion (#155), new capability development, cosmetic refactors, branch/worktree
cleanup and physically unavailable R06 scenarios. No owner process, canonical working file, live
desktop, permission configuration, release admission or version was changed by the audit.

## Evidence discipline

`inventory/hostile-precheck-execution.txt` confirms real execution at the initial SHA: 3 hostile
plugin tests, 1 hostile driver test, 1 protocol-v2 test and 7 federation tests passed with zero ignored
cases. This is PRECHECK evidence, not independent review and not evidence for the later fix commit.
The archive generator's 18 Python tests (12 new + 6 existing packet-contract tests) passed separately.
Each run/job has an exact identifier. QUEUED, IN_PROGRESS, SKIPPED and CANCELLED are never PASS.

## Subsequent reconciliation

See FOLLOWUP_FINDINGS.md: one Windows network backup is behaviorally superseded (47 heads remain),
the Blender provenance fault is identified, Windows ARM64 failure is reproduced at the audit head,
and the website public-proof observation is complete. Initial inventories remain historical snapshots.
