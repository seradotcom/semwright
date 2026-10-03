# R16 defensive review and repository closeout — final R delivery

**R-owned repository work: COMPLETE IN DECLARED SCOPE.**
**Review disposition: REVALIDATION PENDING. Formal R16 status: OPEN.**

This is the final repository/source-review delivery for role R. It is an AI-assisted defensive review, not an external security audit and not a maintainer release decision. R completed the twelve required source-boundary reviews, implemented repository/documentation closeout work, confirmed and fixed one new bounded-resource defect, reran the affected hosted checks, and prepared a reproducible evidence package. The remaining R16 blocker is intentionally external: the R-authored security-relevant fix must be reviewed/adopted by a separate reviewer. Physical and interactive platform gates remain release gates rather than unfinished R source review.

No authority escalation, secret exfiltration, sandbox escape or third-party exploitation was demonstrated in this pass. That statement is not proof that such vulnerabilities do not exist.

## 1. Immutable identities and evidence separation

| Identity | Value |
| --- | --- |
| Frozen review target / main | `6491c0d838fa066938a494524d69ed507aa0dbe8` |
| Final source SHA | `868446205df36826356483e93c53e5060c46e8aa` |
| R-authored product remediation | `4ef9a06e486cd8d2e3851c298e244435ecef3232` |
| PR synthetic merge used by PR jobs | `52c4a7c579c39e5f5699f8b475c473abe2efdcba` |
| PR | [#207](https://github.com/seradotcom/semwright/pull/207) |
| Evidence/report SHA | The Git commit containing this file; deliberately later than FINAL_SOURCE_SHA |
| Formal R16 decision | OPEN; not made by R |

origin/main was rechecked at closeout and still matched `6491c0d838fa066938a494524d69ed507aa0dbe8`. R did not chase a moving target. PR jobs checked out the synthetic merge commit shown above; its Git tree was verified identical to `868446205df36826356483e93c53e5060c46e8aa`. The exact-source R smoke separately checked out the branch commit itself. Historical I/G/native records keep their original SHAs.

The final source commit contains two isolated completion commits after the earlier documentary work:

- `4ef9a06e486cd8d2e3851c298e244435ecef3232` — product fix: bounded federated MCP tools/list pagination.
- `868446205df36826356483e93c53e5060c46e8aa` — R verification harness: positive test inventories and structured fake-effect/audit assertions.

Everything after FINAL_SOURCE_SHA in this delivery is evidence/report/archive material only.

## 2. What R actually changed

The public repository surface was updated rather than replaced. Public or assurance paths changed before the source freeze include:

- `.github/workflows/r16-closeout.yml`
- `CONTRIBUTING.md`
- `DEVELOPMENT_HANDOFF.md`
- `README.md`
- `RELEASE_BLOCKERS.md`
- `SECURITY.md`
- `SUPPORT.md`
- `VERIFY.md`
- `crates/federation/src/bin/fixture.rs`
- `crates/federation/src/lib.rs`
- `crates/federation/tests/federation.rs`
- `docs/architecture.md`
- `docs/compatibility.md`
- `docs/development.md`
- `docs/installation.md`
- `docs/platforms.md`
- `docs/security.md`
- `scripts/review/r16-smoke.py`
- `scripts/review/validate_r16.py`
- `tests/python/test_r16_closeout.py`
- `tests/semantic-adversarial-lab/INTEGRATION.md`

No license file, third-party notice, Cargo.lock, release tag, GitHub release or repository billing setting was changed. No foreign PR was closed or merged.

## 3. New product finding and remediation

### R-006 — federated MCP pagination was bounded too late

Cargo.lock pins rmcp 3.4.1. Its Peer<RoleClient>::list_all_tools() helper appends every page into one vector until next_cursor becomes absent. The pre-fix Semwright path wrapped that helper in a ten-second timeout but applied its 512-tool check only after the helper returned. Therefore an authorized hostile MCP child could make discovery materialize more catalog entries than Semwright intended before its aggregate check executed. The timeout bounded elapsed time, but not incremental catalog materialization or a repeated-cursor loop within that time.

Fix `4ef9a06e486cd8d2e3851c298e244435ecef3232` replaces list_all_tools() with Semwright-owned pagination that:

- refuses a page that would exceed 512 imported tools;
- caps pagination at 512 pages;
- records cursors and rejects a repeated cursor;
- remains inside the existing ten-second discovery timeout;
- adds synthetic fixture modes for 513 tools and repeated cursors.

Current-source hosted validation passed: Native application integration run 37101922029, job 111143008612, compiled semwright-federation and executed 7/7 sandboxed federation tests. The container test invalid_descriptors_duplicate_names_and_bad_results_fail_closed now includes both pagination regressions. Artifact 11266447661 has digest sha256:b72f59d42ce3d2ae7dd05b8ef3ba9b9f5e40e4e3440285167f51ed87a393ffd0.

This is a confirmed product/resource-lifecycle defect with a current-source fix, not a demonstrated authority escalation. Because R authored the behavior/security fix, R does not count its own green CI as independent closure. R-006 remains REMEDIATED_CURRENT_SOURCE_REVALIDATION_PENDING.

## 4. R smoke/evidence defect R-005

The initial R smoke combined libraries for which --lib selected zero tests and primarily relied on exit status. The final harness first executes cargo test ... -- --list, requires a positive count for each selected library, and then executes the selected tests. It also parses the fake CLI JSON and requires the declared effect/audit state.

Actions run 37101919278, job 111143010836, ran on exact source `868446205df36826356483e93c53e5060c46e8aa` and produced:

- policy: 9 listed, 9 passed;
- protocol: 7 listed, 7 passed;
- semantic Composition: 5 listed, 5 passed;
- fake doctor: explicit fake backend;
- ui.find: two ambiguous Save candidates, with no implicit invocation;
- fake recipe: completed, changed=true, both steps successful;
- audit: successful ui.invoke finish record present.

Graph and Effects are deliberately not mislabeled as fresh tests in this smoke. Their source implementations were directly reviewed in the completion pass and historical G evidence retains its own cd518748... identity.

## 5. Twelve mandatory review areas

### R16-01: Authorization and confirmation

**Status:** `DIRECT_SOURCE_REVIEW_COMPLETE_REVALIDATION_PENDING`.

No additional authorization bypass found in the reviewed routes. The review traced owner binding, deny precedence, confirmation separation, post-confirmation revalidation, attempt consumption and reconciliation permits.

**Reviewed invariants:**

- baseline profiles do not silently grant input, clipboard, shell or adapter authority
- explicit deny wins including parameterized filesystem requirements
- sensitive operations require a trusted /dev/tty approval challenge and cannot self-approve through request JSON
- policy is re-enforced and references are re-resolved after approval before dispatch
- PlanVault authority is bound to Owner plus canonical plan bytes and aggregate budget; plan IDs alone are not authority
- partial/unknown effects require explicit exhaustive native reconciliation before repair

**Source locations:** `crates/policy/src/lib.rs:59-183`; `crates/daemon/src/console.rs:13-144`; `crates/core/src/lib.rs:601-1030`; `crates/semantic-composition/src/model.rs:6-26`; `crates/semantic-composition/src/vault.rs:90-542`.

**Limitations:** This is R's repository review, not independent external revalidation of R-authored changes. Application-specific target applications retain their normal OS privileges.

### R16-02: IPC and session identity

**Status:** `DIRECT_SOURCE_REVIEW_COMPLETE_NO_NEW_FINDING`.

Session/request limits, peer authentication and cancellation ownership were directly reviewed. Same-UID malicious processes remain explicitly outside the Unix IPC threat boundary.

**Reviewed invariants:**

- wire frames are bounded before allocation
- request IDs cannot be replayed within one session
- cancellation is scoped to the owning session/connection
- Unix broker socket is private and same-UID authenticated
- Windows named-pipe peers are authenticated from kernel PID/session/SID rather than client JSON
- Windows pipe DACL is owner+SYSTEM and remote clients are rejected

**Source locations:** `crates/protocol/src/lib.rs:15-281`; `crates/daemon/src/server.rs:22-504`; `crates/platform-windows-sys/src/pipe.rs:1-609`.

**Limitations:** Same-UID hostile processes outside Semwright remain a documented non-claim. Interactive Windows behavior remains a separate R18 gate.

### R16-03: Object identity and focus

**Status:** `DIRECT_SOURCE_REVIEW_COMPLETE_NO_NEW_FINDING`.

The dispatch path revalidates stale identity and focus after policy/approval. No implicit input fallback was found.

**Reviewed invariants:**

- references are opaque, session-scoped, TTL-bounded and backend-bound
- backend migration of a reference is forbidden
- native references are validated again immediately before dispatch
- input requires an explicit window reference that remains focused; failure is Conflict with no fallback
- secondary references must share backend/application and are separately validated

**Source locations:** `crates/types/src/lib.rs:250-340`; `crates/core/src/lib.rs:380-500`; `crates/core/src/lib.rs:930-1035`.

**Limitations:** Correctness still depends on each backend's validate/is_focused implementation and platform evidence.

### R16-04: Portal authority

**Status:** `DIRECT_SOURCE_REVIEW_COMPLETE_LIVE_RESIDUAL`.

Source review found owner binding, revocation checks, separate ScreenCast/RemoteDesktop authority and single-use restore-token behavior. Historical isolated portal evidence remains environment-scoped.

**Reviewed invariants:**

- RemoteDesktop, ScreenCast and clipboard authority are separately checked and owner-session scoped
- cross-session stop/capture/use is denied
- portal Closed/revocation state is checked before use
- restore tokens are scope-bound and consumed from local state before reuse
- durable restore state is owner-only, no-symlink, single-link and bounded
- portal stop tears down EIS/clipboard state and provider shutdown closes RemoteDesktop and ScreenCast

**Source locations:** `crates/platform-linux/src/portal.rs:299-1896`; `crates/platform-linux/src/portal_state.rs:1-269`.

**Limitations:** R did not manufacture new physical portal consent sessions. Physical desktop/mixed-display residuals remain R06, not a source-review failure.

### R16-05: Filesystem confinement

**Status:** `DIRECT_SOURCE_REVIEW_COMPLETE_NO_NEW_FINDING`.

Direct source review confirmed three platform-specific mechanisms remain distinct rather than weakened to a portable lowest common denominator.

**Reviewed invariants:**

- Linux uses a pinned root FD plus openat2 RESOLVE_BENEATH/NO_SYMLINKS/NO_MAGICLINKS/NO_XDEV and rejects multi-link files
- Windows uses handle-relative opens, rejects reparse points/cross-volume traversal, pins root identity and verifies atomic-rename identity
- macOS exposes descriptor-relative single-child confinement and explicitly refuses stronger nested claims
- writes are bounded and atomic-or-uncertain; confinement never falls back to string-prefix checks

**Source locations:** `crates/platform-linux-sys/src/filesystem.rs:1-413`; `crates/platform-windows-sys/src/filesystem.rs:164-725`; `crates/platform-macos-sys/src/filesystem.rs:1-131`.

**Limitations:** Configured roots still assume an owner-controlled setup boundary. Blender/application-native file APIs cannot inherit the broker's FD-relative guarantees.

### R16-06: Driver and Plugin isolation

**Status:** `DIRECT_SOURCE_REVIEW_COMPLETE_REVALIDATION_PENDING`.

Host-tool parent binding and persistent session lifecycle were directly reviewed, including cleanup after lost parent/transport. No new escape route was found in the reviewed source.

**Reviewed invariants:**

- driver/plugin executable identity is digest/descriptor bound before use
- host tools require an active authorized parent request and cannot self-create authority
- host-tool calls/jobs/sessions have count, output, frame, timeout and lifetime bounds
- parent cancellation/transport loss cancels work and reaps persistent sessions
- macOS arbitrary driver/plugin execution remains SandboxDenied rather than downgraded

**Source locations:** `crates/driver-host/src/lib.rs:177-381`; `crates/driver-host/src/lib.rs:1930-3115`; `crates/platform-macos-sys/src/launch.rs:55-163`; `crates/plugin-host/src/lib.rs`.

**Limitations:** Linux Bubblewrap/Landlock regressions are not a formal kernel proof. R-authored federation fix still needs independent revalidation before formal R16 closure.

### R16-07: Federated MCP

**Status:** `PRODUCT_DEFECT_FIXED_REVALIDATION_PENDING`.

R-006 was confirmed: rmcp 3.4.1 list_all_tools accumulated pages without an item/page/cursor-cycle bound and Semwright checked 512 only afterward. Commit 4ef9a06e486cd8d2e3851c298e244435ecef3232 replaces it with Semwright-owned bounded pagination and synthetic oversize/cursor-cycle regressions.

**Reviewed invariants:**

- external MCP metadata remains untrusted and namespaced
- every imported tool is privilege-sensitive and still requires the provider policy scope plus operator confirmation
- descriptor digest is rechecked before tools/call
- tool discovery must be bounded while paginating, not after unbounded aggregation
- repeated pagination cursors must fail closed

**Source locations:** `crates/federation/src/lib.rs:452-880`; `crates/federation/src/bin/fixture.rs`; `crates/federation/tests/federation.rs`.

**Limitations:** The product fix is authored by R and therefore cannot count as independently revalidated by R. Current-source hosted federation tests are required before the remediation can be called functionally verified.

### R16-08: Prompt-injection containment

**Status:** `DIRECT_SOURCE_REVIEW_COMPLETE_NO_NEW_FINDING`.

No text-to-authority path was found in the reviewed federation/Skills/broker routes. The project correctly avoids claiming universal LLM prompt-injection immunity.

**Reviewed invariants:**

- untrusted text can describe data but cannot create policy grants
- external MCP descriptions are control-sanitized, bounded and explicitly labeled untrusted
- Skills never auto-execute scripts and requirements reject authority fields
- broker routing/authorization is descriptor/policy based rather than instruction-text based

**Source locations:** `crates/federation/src/lib.rs:575-675`; `crates/skills/src/lib.rs`; `crates/skills/src/package.rs:500-700`; `crates/skills/src/compat.rs`; `docs/skills.md`.

**Limitations:** A separately granted unrestricted shell/desktop tool can bypass this mediated surface. The target application itself may process hostile content with its own privileges.

### R16-09: Audit and disclosure

**Status:** `DIRECT_SOURCE_REVIEW_COMPLETE_REVALIDATION_PENDING`.

Disclosure paths were directly reviewed. Earlier documentation/reporting mismatches were remediated in the R branch; adoption remains independently reviewable.

**Reviewed invariants:**

- audit records typed metadata rather than request/output bodies
- audit chain/line/rotation bounds fail closed on corruption
- failure to persist audit before execution blocks dispatch; failure after effect returns an uncertain outcome
- operator summaries redact sensitive fields and terminal content is escaped
- backend/upstream failure bodies are not copied into trusted error messages

**Source locations:** `crates/core/src/audit.rs:105-315`; `crates/core/src/lib.rs:601-700`; `crates/daemon/src/console.rs:109-144`; `crates/federation/src/lib.rs:760-865`.

**Limitations:** Hash chaining is not tamper-proof against a malicious same-UID process. R's documentation fixes are not an external security attestation.

### R16-10: Resources and lifecycle

**Status:** `PRODUCT_DEFECT_FIXED_REVALIDATION_PENDING`.

R-006 was the one new resource-lifecycle defect found. The source fix adds incremental bounded pagination; other inspected job/session paths were already bounded and fail closed.

**Reviewed invariants:**

- JobStore bounds total/per-session/active jobs and retained result size
- provider progress/artifacts are bounded and terminal jobs cannot mutate
- driver tool output/jobs/sessions have explicit capacities, deadlines and cleanup
- provider disconnect/session revocation cancels owned work
- federated tools/list pagination has item/page/cursor-cycle bounds

**Source locations:** `crates/core/src/jobs.rs:1-474`; `crates/driver-host/src/lib.rs:280-381`; `crates/driver-host/src/lib.rs:1930-3115`; `crates/federation/src/lib.rs:605-675`.

**Limitations:** Independent revalidation is still required for R-006. Hosted regression success does not prove absence of all resource exhaustion paths.

### R16-11: Supply chain

**Status:** `DIRECT_SOURCE_REVIEW_COMPLETE_NO_NEW_FINDING`.

R scanned all workflow uses entries in the candidate and found zero unpinned external Actions. Release admission currently exits 2 because live_desktop_matrix and security_review remain false.

**Reviewed invariants:**

- Cargo.lock is committed and release admission requires a parseable locked graph
- workflow actions are pinned to immutable 40-hex SHAs
- release-readiness has an independent required-gate set and remains fail closed while security/live gates are false
- package checksums/attestations are not represented as universal publisher identity

**Source locations:** `Cargo.lock`; `rust-toolchain.toml`; `.github/workflows/*.yml`; `scripts/release/assert-ready.py`; `release-readiness.json`; `deny.toml`.

**Limitations:** Open Dependabot update PRs remain maintainer work and were not merged by R. Current dependency/fuzz/coverage jobs may legitimately skip when their source area is unaffected; skips are not PASS evidence.

### R16-12: Platform boundaries

**Status:** `DIRECT_SOURCE_REVIEW_COMPLETE_LIVE_RESIDUAL`.

Direct source review found explicit platform-specific fail-closed behavior. Documentation now describes Windows as implemented/native-tested but not interactively certified and macOS arbitrary driver/plugin execution as unavailable.

**Reviewed invariants:**

- Linux confinement is not weakened to emulate another platform
- macOS executable verification does not imply driver/plugin sandbox acceptance; arbitrary child isolation remains disabled
- Windows uses native handle-relative filesystem and named-pipe/AppContainer authority rather than Linux emulation
- hosted native tests are distinguished from interactive TCC/UAC/display certification

**Source locations:** `crates/platform-services/src/lib.rs`; `crates/platform-linux-sys/src/filesystem.rs`; `crates/platform-macos-sys/src/launch.rs`; `crates/platform-macos-sys/src/filesystem.rs`; `crates/platform-windows-sys/src/filesystem.rs`; `crates/platform-windows-sys/src/pipe.rs`; `docs/platforms.md`.

**Limitations:** R06 physical Hyprland/mixed-display coverage remains external to this source review. R18 unlocked Windows interactive certification remains external to this source review. Authorized live macOS TCC acceptance remains separate from hosted build evidence.

## 6. Cross-system Composition → Broker → Host → application → artifact → consumer → Graph/Effects review

The completion pass explicitly traced the cross-system invariants that were missing from the earlier partial report:

- PlanVault binds owner/session/principal, canonical plan bytes, root incarnation and aggregate budget; a plan ID is not authority. Attempts are consumed before effects and partial/unknown outcomes cannot be repaired without a bounded, native, exhaustive reconciliation.
- Project Graph starts knowledge as UNKNOWN, requires safe coverage and observed determinants to reach CURRENT, and maps denied/offline/ambiguous probes without fabricating freshness. Declared edges report execution_certified=false; imported manifests remain declarations_only.
- Effect evaluation requires a current base/context, independent observed readback, exhaustive scope where required, matching method/source/artifact and a true predicate before PASS.
- AV native receipts are request/owner/plan/stage/proof-bound. Partial/unknown receipts block publication, missing receipts transition state to Unknown without retry, and publication must match the previously prepared manifest digest and destination pointer.
- Skills remain procedural knowledge, not execution authority; bundled scripts may be inspected or packaged but Semwright does not auto-execute them.

No new cross-system bypass was identified beyond R-006.

## 7. Current-source CI and evidence

| Evidence | Actual checkout | Result | What it establishes |
| --- | --- | --- | --- |
| R16 documentary + positive smoke 37101919278 / 111143010836 | `868446205df36826356483e93c53e5060c46e8aa` | PASS | locked daemon/CLI build, 21 positive selected library tests, structured fake effect/audit |
| Quality source contracts 37101922062 / 111143005458 | `52c4a7c579c39e5f5699f8b475c473abe2efdcba` (tree = final source) | PASS | 200 Python tests, 20/20 Node tests, OSS hygiene |
| Quality static lints 37101922062 / 111143005373 | `52c4a7c579c39e5f5699f8b475c473abe2efdcba` (tree = final source) | PASS | Ruff F/E9, ShellCheck, actionlint |
| Driver conformance 37101922029 / 111143008612 | `52c4a7c579c39e5f5699f8b475c473abe2efdcba` (tree = final source) | PASS | 7/7 sandboxed federation tests including R-006 regressions |

Jobs excluded by affected-area classification remain SKIPPED, not PASS. In particular, this completion pass does not invent fresh MSRV/full-workspace, coverage or fuzz results when their source areas were not selected. Historical evidence is cited only with its original SHA/scope.

The release guard remains deliberately fail closed: release-readiness.json is BLOCKED_DEVELOPMENT_SOURCE, with live_desktop_matrix=false and security_review=false; scripts/release/assert-ready.py exits 2 on the final source.

All external GitHub Actions uses entries in the candidate were scanned; zero floating/unpinned external Actions were found (each uses an exact 40-hex commit).

CircleCI was not used because no authorized CircleCI CLI/token/connector was available to this session. No credentials were requested. Standard GitHub-hosted runners were sufficient for the selected current-source lanes. No heavy Rust/native build was executed on the owner device.

## 8. Findings disposition

| ID | Severity | Status | Blocks R review closure? | Blocks release? |
| --- | --- | --- | ---: | ---: |
| R-001 | `MEDIUM` | `REMEDIATED_FUNCTIONALLY_VERIFIED` | no | no |
| R-002 | `MEDIUM` | `REMEDIATED_CURRENT_SOURCE_VALIDATED_REVALIDATION_PENDING` | no | no |
| R-003 | `MEDIUM` | `REMEDIATED_CURRENT_SOURCE_VALIDATED_REVALIDATION_PENDING` | no | no |
| R-004 | `LOW` | `REMEDIATED_FUNCTIONALLY_VERIFIED` | no | no |
| R-005 | `MEDIUM` | `REMEDIATED_FUNCTIONALLY_VERIFIED` | no | no |
| R-006 | `MEDIUM` | `REMEDIATED_CURRENT_SOURCE_REVALIDATION_PENDING` | yes | no |
| R-007 | `HIGH_ASSURANCE_GAP` | `BLOCKED_REVALIDATION` | yes | yes |
| R-008 | `MEDIUM_ASSURANCE_GAP` | `REMEDIATED_DIRECT_REVIEW_COMPLETE` | no | no |
| R-009 | `MEDIUM_GOVERNANCE_GAP` | `OPEN_MAINTAINER_DECISION` | no | no |
| R-010 | `RELEASE_GATE` | `OPEN_KNOWN_LIMITATION` | no | yes |

R-008 is closed for the declared source-review scope: all twelve areas were directly inspected. R-010 remains the already-known physical/interactive platform gate. R-009 remains a maintainer governance choice because no main branch protection/ruleset was observed and R did not alter repository settings.

The two review-closure blockers are:

1. **R-006:** the product fix is green but needs a separate reviewer because R authored it.
2. **R-007:** R provenance/assurance changes must be independently adopted before a formal R16 decision.

These are not additional implementation tasks that R can honestly self-complete.

## 9. Documentation/repository closeout

The repository now has an updated first-run path, installation guide, architecture, platform support matrix, security/reporting guidance, compatibility/development guidance and R16 evidence entry point. The quickstart preserves the committed lockfile and no longer invokes the lock initialization bootstrap on an ordinary checkout. Windows/macOS/Linux support statements distinguish implementation, hosted native evidence and interactive/physical certification.

The selectively consulted 199-biotechnologies/github-optimization-skill was treated only as an editorial reference. R adopted scannability/quickstart structure where truthful, but did not install the skill, add star/follow CTAs, change repository metadata, replace licensing, auto-publish, or import unsupported promotional/growth claims.

## 10. PR and branch disposition

| PR | Disposition |
| --- | --- |
| #207 | R-owned closeout; ready for maintainer/independent review. Do not auto-merge. |
| #166 | Preserve open; dependency update outside R mission. |
| #165 | Preserve open; dependency update outside R mission. |
| #161 | Preserve open; dependency update outside R mission. |
| #157 | Preserve open; Action dependency update outside R mission. |

No foreign PR was closed, rebased, merged or overwritten. Existing merged/closed historical PRs remain Git history/evidence and were not deleted to make the repository look cleaner.

## 11. Completion status and maintainer-only decisions

All work that role R can perform under this mission is complete. CLOSEOUT_STATUS.json records all sixteen closeout acceptance items as satisfied within their declared scope while preserving the difference between repository completion and formal R16/release authority.

1. Assign an independent reviewer to review/adopt R-authored security-relevant changes, especially fix 4ef9a06e486cd8d2e3851c298e244435ecef3232, before deciding formal R16 closure.
2. Choose/configure or explicitly risk-accept the absence of main branch protection/rulesets; R did not change repository settings.
3. Review and merge PR #207 only if acceptable; R does not merge main.
4. Complete R06 physical desktop and R18 unlocked Windows (plus any desired live macOS TCC) release gates before making corresponding platform/release claims.
5. Handle Dependabot PRs #157/#161/#165/#166 separately; R neither merged nor closed them.

R does not merge main, publish a release, close R16, or represent itself as an external auditor.

## 12. Machine-readable companion files

- [`R16_FINDINGS.json`](R16_FINDINGS.json)
- [`CLAIMS_EVIDENCE_MATRIX.json`](CLAIMS_EVIDENCE_MATRIX.json)
- [`R16_EVIDENCE_MANIFEST.json`](R16_EVIDENCE_MANIFEST.json)
- [`CLOSEOUT_STATUS.json`](CLOSEOUT_STATUS.json)
- [`PR_BRANCH_DISPOSITION.json`](PR_BRANCH_DISPOSITION.json)
- [`evidence/R_DIRECT_REVIEW_12_AREAS.json`](evidence/R_DIRECT_REVIEW_12_AREAS.json)
- [`evidence/R_CURRENT_SOURCE_VALIDATION.json`](evidence/R_CURRENT_SOURCE_VALIDATION.json)
- [`SHA256SUMS`](SHA256SUMS)

The final ZIP/manifest under delivery/ is a backup of the reviewed delta/evidence, not a Git release and not a substitute for repository history.
