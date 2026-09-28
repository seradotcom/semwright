> **Historical first-pass report.** Current continuation, owner assignments and admission status are in [R16_HANDOFF_READINESS.md](R16_HANDOFF_READINESS.md), [OWNER_HANDOFF.md](OWNER_HANDOFF.md), [BRANCH_DISPOSITION_CLOSEOUT.md](BRANCH_DISPOSITION_CLOSEOUT.md) and `pre-r16-audit.json`. Earlier snapshots and failed logs below are preserved, not presented as current-head certification.

# Security precheck — twelve R16 areas

Baseline inspected: `241000c268d1bf1dc29d4e91a913097ac0d020cb`. **Maintainer PRECHECK only; R16 remains OPEN.**
`PRECHECK PASS` below is confined to the expressly listed source/fixture checks. It does not mean
independent review, full attack-surface coverage, or execution of the audit branch's new code.
`NOT EXECUTED` records missing dynamic work even when supporting historical/static evidence exists.

## 1. Authorization and confirmation

**PRECHECK PASS — Scoped static/fixture precheck only.**

Sources: `crates/policy/src/lib.rs; crates/core/src/lib.rs; crates/daemon/src/console.rs; broker_contract/provider_runtime tests`.

Deny precedence, explicit filesystem/application scopes, broker policy re-entry, independent foreground-console challenge and post-approval revalidation exist. Request metadata cannot manufacture approval. Full interactive hostile-prompt testing was not repeated.

## 2. IPC and session identity

**PRECHECK PASS — Scoped static/fixture precheck only.**

Sources: `crates/daemon/src/server.rs; crates/core/src/jobs.rs; broker_contract/provider_runtime tests`.

Peer validation precedes admission; session/request IDs are bounded and replay IDs rejected. Connection cancellation and job refs/events are session-scoped. Broker sessions share an owner policy; this is not a mutually hostile same-UID tenant service.

## 3. Object identity and focus

**FINDING — Windows exact-SHA failure remains.**

Sources: `crates/core/src/lib.rs; crates/platform-windows/tests/uia_native_fixture.rs; Actions 36394993424/108839165329`.

Broker dispatch resolves refs after approval, checks backend identity, and refuses input unless the window remains focused. No silent fallback is taken. Windows ARM64 nevertheless fails exact UIA inspection with StaleReference; root cause and regression remain owner work.

## 4. Portal authority

**NOT EXECUTED — No new live portal session in this audit.**

Sources: `crates/platform-linux/src/portal.rs; verification/live-portal-eis/; RELEASE_BLOCKERS.md R02/R04/R06`.

Historical isolated VM input and private protocol fixtures are distinguished from invalidated shared-authority attempts. A successful initial-SHA PipeWire job is recorded, not promoted to physical login/multimonitor proof. No consent dialog or input was driven on the connected owner desktop.

## 5. Filesystem confinement

**PRECHECK PASS — Scoped Linux source and existing regression review.**

Sources: `crates/platform-linux-sys/src/filesystem.rs; crates/platform-common/src/artifact.rs`.

FD-relative openat2 rejects symlinks, mount crossing, dot/parent paths; reads require bounded single-link regular files; atomic writes use private exclusive temporaries and directory fsync. Artifact copy enforces source/destination grants and digest before write. This is not an exhaustive root-open race proof or equivalent Windows/macOS certification.

## 6. Plugin and driver isolation

**PRECHECK PASS — Executed initial-SHA Linux hostile fixtures.**

Sources: `crates/driver-host/src/lib.rs; crates/platform-linux-sys/src/launch.rs; inventory/hostile-precheck-execution.txt`.

Linux launcher clears environment, requires Bubblewrap/Landlock, materializes explicit mounts/tools and has no unsandboxed fallback. Hostile plugin 3/3 and driver 1/1 actually ran; protocol-v2 1/1 also ran. Driver/native application authority and unrelated same-UID processes remain distinct boundaries. Blender runtime changes are unresolved owner work.

## 7. Federated MCP

**PRECHECK PASS — Executed initial-SHA federation fixture.**

Sources: `crates/federation/src/lib.rs; inventory/hostile-precheck-execution.txt`.

Sandboxed stdio fixture ran 7/7 tests. Definitions/identities are owner-bound, descriptors are untrusted and namespaced, policy is re-entered, and disconnect invalidates catalog generation. Remote transports and Windows filesystem virtualization are not implied. README/VERIFY sandbox claims were stale and are corrected in the audit branch.

## 8. Prompt-injection containment

**PRECHECK PASS — Code/contract-level authority review, not LLM persuasion.**

Sources: `crates/core/src/lib.rs; crates/core/src/workflows.rs; crates/registry/src/; crates/core/tests/provider_runtime.rs`.

Imported metadata does not grant builtin provenance or self-approval. Workflow proposals are non-executing until explicit gates; learned recipe steps re-enter policy. Completed workflow-library visibility is owner-wide, not a separate tenant vault; recording/event ownership does not imply cross-user isolation. Formal information-flow noninterference is not claimed.

## 9. Audit and disclosure

**FINDING — CLI remediation committed; new Rust execution pending.**

Sources: `crates/core/src/audit.rs; crates/core/src/lib.rs; crates/cli/src/lib.rs; crates/cli/tests/terminal_display.rs`.

Journal records metadata rather than raw request values; audit-finish failure returns an uncertain outcome instead of claiming rollback. CLI terminal controls required escaping (PRE-003). Tracked-pattern scan found no high-confidence candidate and GitHub open alert metadata was empty; this was not full-history secret scanning or comprehensive TUI/error-path proof.

## 10. Resource and lifecycle safety

**NOT EXECUTED — No new whole-runtime soak.**

Sources: `RESOURCE_LIFECYCLE_AUDIT.md; crates/core/src/jobs.rs; crates/daemon/src/server.rs; existing hostile fixtures`.

Session/job/result/event budgets and cancellation/shutdown paths were inspected. Existing hostile tests cover descendant cleanup. Long-repetition RSS/FD/socket measurements, browser crash/restart soak and all cross-platform cleanup races were not rerun; no new absence-of-leaks claim is made.

## 11. Supply chain

**FINDING — Release-tool remediation and evidence gates pending.**

Sources: `scripts/dev/security-review-bundle.sh; .github/workflows/; Cargo.lock; deny.toml`.

All 199 observed Action uses are immutable SHAs/local actions. Scoped attestation job excludes PRs and requires Nix/bundle jobs; it uses contents:read plus id-token/attestations/artifact-metadata write, not write-all. Bundle deletion and BASELINE_SHA checksum defects were fixed and 18 packet tests passed. Hosted dependency/coverage/fuzz jobs succeeded at the initial SHA; no independent certification follows.

## 12. Platform boundaries

**FINDING — Windows regression and incomplete owner work.**

Sources: `docs/compatibility.md; inventory/main-runs-start.json; PR153/154`.

Linux, macOS and Windows have different enforcement and evidence. Initial Windows ARM64 failed; hosted successes are not interactive acceptance. Mac TCC, physical R06 and unresolved security-related branch/worktree work prevent an unqualified freeze. Owners retain their active work.

## Reviewer exclusions and follow-up

All 389 unsafe-marker hits were inventoried, but were not individually proven memory-safe.
Platform FFI lengths/ownership, macOS/Windows native cleanup, dynamic network port reuse,
full provider crash/soak behavior and secret-scanner history coverage require further review.
A reviewer must use the eventual main candidate SHA, not this initial observation or an audit branch.
