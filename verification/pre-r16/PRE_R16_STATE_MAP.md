# Pre-R16 state map

## Decision

**NOT_READY_FOR_INDEPENDENT_R16_REVIEW**. R16 is OPEN. No candidate SHA has been declared.
This is an interim maintainer preflight record, not an independent security assessment.

## Snapshot

- AUDIT_START_SHA: `241000c268d1bf1dc29d4e91a913097ac0d020cb`; initial observation 2026-09-28, after one origin fetch.
- Audit branch: `audit/pre-r16-forensic`; remediation commit: `53c5344ec3fea500b7cade8471928a25bb194048`.
- Canonical checkout was on the Blender branch with untracked verification evidence. It was not modified.
- The audit uses its own worktree. Windows, Blender/Godot and their Git state remain owner-managed.
- Metadata/environment: `inventory/snapshot.json`. All heavy validation goes to GitHub Actions.

## Current main evidence

At the initial exact SHA, 12 workflows completed: 11 success and Windows failure.
Windows ARM64 failed exact UIA inspection with `StaleReference`; later steps in that job were skipped.
Neither successful x64 checks nor earlier green runs substitute for this failed ARM64 gate.
Run/job records: `inventory/main-runs-start.json` and `inventory/run-*-jobs.json`.

## Open PRs at the later observation

| PR | Classification | Disposition |
|---|---|---|
| #153 | F — BROKEN / INCOMPLETE | Windows owner: fixture failure, positive test-count admission, dedicated interactive runner |
| #154 | F — BROKEN / INCOMPLETE | Blender owner: runtime pinning/security subset and failing checks; avoid unnecessary GLB surface expansion |
| #155 | E — POST-RELEASE | TIDELING demo is outside this freeze; reconcile overlapping Blender export with #154 |

`must_land_prs` is intentionally empty: no mixed/incomplete PR has been admitted wholesale.
This does **not** mean there is no required remediation; #153/#154 and this audit's fixes must be resolved.

## Archaeology

333 branch refs, 188 worktrees, 46 dirty worktrees and 14 detached worktrees were inventoried.
85 refs are ahead of the initial main. Of 63 distinct unmerged heads, 15 are patch-equivalent or
have identical changed paths in main; 48 still need behavioral disposition. Every worktree is KEEP.
The 154-PR historical inventory contains 130 merges, 22 closures without merge and 2 then-open PRs;
#155 appeared later and is included in the classified open inventory. A closed PR is not proof of integration.

## Release documents and blockers

README, VERIFY, compatibility and changelog corrections are in the audit branch. The obsolete
PipeWire doctor field is fixed. `RELEASE_BLOCKERS.md` remains under concurrent Windows owner work;
its evergreen green-CI/R11 sandbox wording is still a coordination item, not silently assumed corrected.
R06 preserves physical Hyprland/mixed-display limitations. R16 requires a genuinely independent review.
No release version, gate admission, tag, branch deletion or canonical checkout cleanup was performed.

## Subsequent reconciliation

See FOLLOWUP_FINDINGS.md: one Windows network backup is behaviorally superseded (47 heads remain),
the Blender provenance fault is identified, Windows ARM64 failure is reproduced at the audit head,
and the website public-proof observation is complete. Initial inventories remain historical snapshots.
