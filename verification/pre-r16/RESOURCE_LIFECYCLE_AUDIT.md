> **Historical first-pass report.** Current continuation, owner assignments and admission status are in [R16_HANDOFF_READINESS.md](R16_HANDOFF_READINESS.md), [OWNER_HANDOFF.md](OWNER_HANDOFF.md), [BRANCH_DISPOSITION_CLOSEOUT.md](BRANCH_DISPOSITION_CLOSEOUT.md) and `pre-r16-audit.json`. Earlier snapshots and failed logs below are preserved, not presented as current-head certification.

# Resource and lifecycle audit

Source baseline: `241000c268d1bf1dc29d4e91a913097ac0d020cb`. No new resource leak was reproduced, but an absence of leaks is **not** established.

| Resource | Inspected mechanism / evidence | Remaining |
|---|---|---|
| Broker sessions and requests | 32 retained sessions, 16 active/session, 4096 replay IDs; 64 connections; timed request completion and connection-owned cancellation | repeated connect/disconnect and idle expiry soak |
| Jobs and results | 256 global jobs, 64/session, 16 active/session; terminal eviction and 262144-byte retained result cap; revocation removes/cancels owner jobs | long-operation crash/reconnect soak and RSS trends |
| Events/subscriptions | source/session audience filtering, bounded retained events, subscription abort on connection close | repeated subscriber churn, fd/task accounting |
| Linux driver/plugin descendants | Bubblewrap die-with-parent/new-session and kill-on-drop construction; executed hostile timeout/descendant fixtures | independent platform-wide lifecycle review, native application behavior |
| Staged binaries and tools | RAII staged files/owned descriptors; Windows host-mediated tools clear inherited root grants | fault injection of staging/removal errors and shared tool accounting |
| Filesystem/artifact temporaries | exclusive 0600 temporary files, FD-relative writes, fsync and confined rename; checksum mismatch rejected before output | interrupted writes and failure-injection cleanup across OSes |
| Review bundles | exclusive private output; never overwrite/delete an existing packet; repeat-generation/tamper tests executed | exact final candidate packet after integration, not before |
| Browser profiles/downloads | initial-SHA Chromium workflow success and existing quota/crash/cleanup regression descriptions | log-level reconfirmation and repeated production browser lifecycle measurement |
| Portal/EIS/PipeWire | historical explicit stop/cancellation and current hosted synthetic stream job | new isolated live authority run where appropriate; never use owner desktop for soak |
| Worktrees/verification evidence | all 188 preserved; dirty and process snapshot recorded | owner-led disposition; process snapshot is not a cleanup certificate |

## Cancellation semantics

The broker reports uncertain outcomes when cancellation/provider loss races a side effect.
An audit-write failure after an operation also reports uncertainty; it does not assert rollback.
Recipes are not transactions. Job cancellation bypasses the mutation execution gate so cancellation
can reach a blocked provider, but still enforces session ownership and normal policy.

## What was not measured

No hundred-iteration RSS/FD/socket/temp-directory trend, process-descriptor census after dynamic
provider crashes, full-application profile leak test or cross-platform fault-injection campaign was
performed by this audit. Existing hosted test outcomes are supporting evidence only. No owner
processes were terminated and no historical artifacts/worktrees were removed.
