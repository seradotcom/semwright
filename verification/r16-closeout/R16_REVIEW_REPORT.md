# R16 defensive review and repository closeout

**Disposition: PARTIAL REVIEW; FINDINGS OPEN; INDEPENDENT REVALIDATION PENDING.**
**Documentation: implemented and verified within the updated public surface. R16: OPEN.**

This is an AI-assisted repository review by role R, not an external security audit. It
combines defensive code/contract inspection, source-specific evidence verification and
actual documentation changes. It does not claim completion of every mandatory code-level
inspection. The missing work and its impact are recorded below rather than converted into
success. No confirmed exploitable product vulnerability was established; this is not proof
of absence of vulnerabilities.

## 1. Identity, source admission and independence

| Identity | Value |
| --- | --- |
| Review ID | `R16-2026-10-03-6491c0d8` |
| Reviewed product/main source | `6491c0d838fa066938a494524d69ed507aa0dbe8` |
| Integrated engineering source from I | `cd518748f742025a251b78028613aa1b16919e73` |
| R smoke suite | `df17366855be487fc22189658f6a3f0cc0f5dfc5` |
| Final documentary/verification source candidate | `206ce2477e8cd02aa66a876328d25a74cf675a3c` |
| PR source/static test checkout | `21447095a4120fc52f2506e8d7e4ffcf9e7562fd` |
| Candidate and PR merge tree | `999f95cb4b4e4fb644b8df176cff7441e7182965` |
| Delivery commit | Recorded after creation in the backup's external `DELIVERY_MANIFEST.json` |
| PR | [#207, R-owned draft](https://github.com/seradotcom/semwright/pull/207) |
| Independence eligibility | UNCONFIRMED; no separate reviewer adoption is present |

The authorized device, canonical checkout, worktrees, current main, open PRs and relevant
closed PRs were inspected. The actual main snapshot was frozen once for product review;
a later metadata check still found the same main SHA. Historical PRs were not presumed
open. R used a separate `audit/r16-release-closeout` branch and worktree.

I's consumable engineering declaration and current native Windows evidence were available.
The observed I-to-main diff contains four documentary files and the Windows UIA fixture,
not changes to production code. This preserves the usefulness of I/G evidence without
relabeling their executions. I's original failed global run and corrected external-fixture
retest retain separate identities. Technical readiness, H evaluation and R16 are different
states. H remains deferred and was not executed by R.

R did not author a product Rust fix in this mission. R did author documentation, a workflow,
a smoke harness and its documentary validator. Those assurance changes cannot independently
validate themselves. A new R session and green CI are not sufficient evidence of external
independence; the maintainer must assess and adopt the review with a separate reviewer.

## 2. Method, evidence boundaries and unavailable work

R traced selected broker, policy, IPC, reference/focus, filesystem, Host, federation, audit,
job and platform-composition paths. The exact successfully read ranges and original Git
blob identities appear in [the evidence manifest](R16_EVIDENCE_MANIFEST.json). A listed
range is not a claim that the entire file or every platform implementation was audited.
R also reviewed the declared application/Composition/AV/Graph/Effects contracts and I/G
engineering receipts. No new hostile payload, escape campaign or exploit reproduction was
executed, and no production secret or private application profile was used.

Several additional read requests involving PlanVault, Graph/Effects and portal ranges were
blocked by the tool service. Additional test-target selection and a proposed R-owned smoke
refinement were also blocked; those refinements were not applied. R did not bypass these
blocks or mark the missing inspections as passing. Other safe source reads, receipt checks
and documentation work continued. These are actual coverage limits, not an assertion that
the affected product components are defective.

Fresh execution is intentionally bounded. The hosted R smoke tested the frozen product
source, while PR source/static jobs checked a synthetic merge. The latter's tree equality
with the source candidate was verified through GitHub Git-commit metadata and checkout
logs. Classifier jobs, skipped jobs, lab self-tests, product contract cases and native
application cases are counted separately. No historical result is silently inherited by
a report commit.

## 3. Twelve mandatory review areas

### R16-01: Authorization and confirmation

**Status:** `PARTIAL_REVIEW`. **Owner:** Broker / Composition owners.

Schemas, explicit grants, application scope, operator confirmation and post-confirmation reference/policy checks converge on broker dispatch. Nine policy tests and five Composition library tests ran in R smoke.

**Evidence locations:** `crates/policy/src/lib.rs:1-382`; `crates/core/src/lib.rs:591-1000`; `crates/daemon/src/console.rs:1-145`.

**Limit:** Direct PlanVault/prepared-plan lifetime inspection was tool-blocked. A passing library subset does not cover every cross-domain owner/revocation transition.

Related findings: `R-007`, `R-008`.

### R16-02: IPC and session identity

**Status:** `REVIEWED_NO_OPEN_BLOCKING_FINDINGS_IN_SCOPE`. **Owner:** IPC / platform owners.

Bounded framed IPC, peer validation, private endpoints/tickets, session expiry and request replay rejection are present. Seven protocol library tests ran. Windows server dispatch explicitly validates the peer.

**Evidence locations:** `crates/daemon/src/server.rs:1-320`; `crates/daemon/src/server.rs:435-506`; `crates/protocol/src/lib.rs:1-145`; `crates/protocol/src/lib.rs:236-286`.

**Limit:** Same-user hostile processes remain outside the stated boundary. R did not independently reexecute Windows principal or interactive-session cases.

### R16-03: Object identity and focus

**Status:** `PARTIAL_REVIEW`. **Owner:** Broker / Graph / desktop owners.

Broker references are resolved again after approval; live validation and focused-window requirements precede input. The Graph identity contract is distinct from permission. G Graph receipts contain 74 product cases on I source.

**Evidence locations:** `crates/core/src/lib.rs:591-1000`; `docs/project-graph/INTEGRATION.md`; `docs/semantic-creation/INTEGRATION.md`.

**Limit:** Direct Graph implementation inspection and additional test selection were blocked. R Graph --lib executed zero tests. A focus precheck is not a proof of exclusive graphical authority.

Related findings: `R-005`, `R-008`.

### R16-04: Portal authority

**Status:** `BLOCKED_ENVIRONMENT`. **Owner:** Linux portal owner / maintainer.

Reviewed the declared OS/user-consent, restore-token, clipboard and session-lifecycle contract and the boundaries of historical desktop evidence. Earlier shared-authority keyboard diagnostics remain invalidated; later isolated VM records are not physical-hardware certification.

**Evidence locations:** `docs/permissions.md`; `VERIFY.md`; `RELEASE_BLOCKERS.md`; `verification/live-portal-eis/`.

**Limit:** The requested portal implementation ranges were tool-blocked. R did not request live portal consent or reexecute any interactive portal sequence. Code-level and physical residuals remain explicit.

Related findings: `R-008`, `R-010`.

### R16-05: Filesystem confinement

**Status:** `PARTIAL_REVIEW`. **Owner:** Filesystem / platform owners.

The inspected Linux path uses a pinned directory descriptor, relative-path validation and openat2 confinement. Reads are bounded regular single-link files; publication uses a private temporary file and descriptor-relative parent. Observation explicitly states it is not CAS.

**Evidence locations:** `crates/platform-linux-sys/src/filesystem.rs:1-320`; `crates/platform-services/src/lib.rs:1-250`.

**Limit:** R reviewed the Linux slice and platform dispatch, not every implementation line on all three hosts. No new live filesystem race or platform-native campaign ran. Existing metadata/hash identity is not a universal transactional guarantee.

Related findings: `R-008`.

### R16-06: Driver and Plugin isolation

**Status:** `PARTIAL_REVIEW`. **Owner:** Driver Host / Plugin / platform owners.

Typed Host arguments check mount/dependency allowlists, and the inspected Linux launch route delegates to the platform sandbox. The Host declares bounded calls, jobs, sessions, frame output and session lifetime. Provider-scoped sessions differ from user-session-scoped jobs.

**Evidence locations:** `crates/driver-host/src/lib.rs:166-340`; `crates/platform-services/src/lib.rs:1-250`; `docs/runtime-tools.md`.

**Limit:** The large Host/Plugin implementation was not exhaustively audited. Existing-process application authority is distinct from child sandboxing. R did not run hostile payloads or a new native confinement campaign.

Related findings: `R-007`, `R-008`.

### R16-07: Federated MCP

**Status:** `FINDINGS_OPEN`. **Owner:** Federation / broker owners.

Imported tools have owner-bound namespace, untrusted descriptions, explicit provider requirements and mandatory sensitive confirmation. Descriptor digest drift is rejected; cancellation/upstream failures preserve uncertainty.

**Evidence locations:** `crates/federation/src/lib.rs:551-870`; `crates/core/src/lib.rs:591-1000`.

**Limit:** The 512-tool count check follows list_all_tools completion. Cumulative SDK pagination/frame allocation bounds were not established in this review; R-006 is unconfirmed, not a demonstrated exploit.

Related findings: `R-006`.

### R16-08: Prompt-injection containment

**Status:** `REVIEWED_NO_OPEN_BLOCKING_FINDINGS_IN_SCOPE`. **Owner:** Broker / Skills / frontend owners.

Upstream descriptions are labeled untrusted and bounded; the inspected confirmation path escapes resource text. The broker validates descriptors/grants independently of prose. Skills and recipes are documented as procedures, not extra authorization.

**Evidence locations:** `crates/federation/src/lib.rs:551-870`; `crates/core/src/lib.rs:591-1000`; `crates/daemon/src/console.rs:1-145`; `docs/skills.md`.

**Limit:** No universal prompt-injection immunity is claimed. This is deterministic authority-path review, not a model-based robustness benchmark, and not an audit of every renderer or third-party application.

### R16-09: Audit and disclosure

**Status:** `REMEDIATED_REVALIDATION_PENDING`. **Owner:** Audit / maintainer.

The inspected audit path preserves provenance, validates an existing chain, bounds retention and records abandoned/unknown outcomes. R observed successful nested invocation metadata in the fake smoke. Public support/security instructions now match the enabled private reporting channel.

**Evidence locations:** `crates/core/src/audit.rs:106-316`; `crates/core/src/lib.rs:591-1000`; `SECURITY.md`; `SUPPORT.md`.

**Limit:** The chain is not a publisher signature or protection against an already-authorized same-user process rewriting state. R-authored security and evidence guidance still requires independent adoption.

Related findings: `R-003`, `R-004`, `R-007`.

### R16-10: Resources and lifecycle

**Status:** `FINDINGS_OPEN`. **Owner:** Jobs / Federation / Host owners.

Jobs have explicit global/session/active/result/artifact limits and owner-filtered get/list/cancel. Session IPC and Host tool budgets are visible. Historical G lifecycle receipts record 18 cases; they were not rerun by R.

**Evidence locations:** `crates/core/src/jobs.rs:1-260`; `crates/daemon/src/server.rs:1-320`; `crates/driver-host/src/lib.rs:166-340`; `crates/federation/src/lib.rs:551-870`.

**Limit:** The unconfirmed federated aggregate-list observation remains open. Script completion is not comprehensive descendant/process/socket cleanup proof; unsupported native platform resource combinations retain their limits.

Related findings: `R-006`.

### R16-11: Supply chain

**Status:** `PARTIAL_REVIEW`. **Owner:** Supply-chain / maintainer.

The committed lock and pinned toolchain were preserved. R workflow actions are immutable-pinned with read-only contents permission and separate source/suite checkout. Fourteen historical G archive digests and four I/Windows archive digests were matched. The release guard still rejects missing live/security admission.

**Evidence locations:** `Cargo.lock`; `rust-toolchain.toml`; `.github/workflows/r16-closeout.yml`; `scripts/ci/pre-r16-secret-scan.py:1-193`; `scripts/release/assert-ready.py:1-115`.

**Limit:** No fresh full dependency, secret-history, packaging, SBOM, Nix, notarization or signature campaign ran on the documentary candidate. Existing I certificates retain their source identity. A digest proves matching bytes, not publisher identity.

Related findings: `R-007`, `R-009`.

### R16-12: Platform limits

**Status:** `PARTIAL_REVIEW`. **Owner:** Platform owners / maintainer.

The platform dispatch selects separate Linux/macOS/Windows services; macOS arbitrary-child sandbox entry remains denied. The current Windows native run is successful at the review target, and public docs no longer describe Windows as only future work.

**Evidence locations:** `crates/platform-services/src/lib.rs:1-250`; `docs/platforms.md`; `docs/compatibility.md`; `RELEASE_BLOCKERS.md`.

**Limit:** Hosted native CI is not unlocked-desktop acceptance. Physical R06, Windows R18 and interactive macOS/TCC scope were not exercised by R. No Linux PASS is extrapolated to another OS.

Related findings: `R-002`, `R-010`.

## 4. Cross-domain composition review

The contract chain is Composition -> broker -> Host -> native application -> artifact ->
consumer -> Graph/Effects. The inspected broker keeps the grant/confirmation boundary;
persistent identities and provider prose are not new grants. The reviewed Host slice
constrains typed mount/dependency arguments. I's joint acceptance records native Blender
assets entering a managed Godot project and then a Motion/AV/Graph evidence path. This
is not Godot movie footage, a commercial demo or an R-native reexecution.

G's historical receipts cover 70 Composition, 35 AV, 74 Graph and 40 Effects contract cases
on I's product source. The verified original receipt limits remain in the normalized
record, including their distinction from combined-wave/native acceptance. The receipt
review does not replace the tool-blocked direct inspection of prepared plans, Graph and
Effects. R therefore does not issue a complete cross-domain security verdict. In particular,
UNKNOWN/incomplete enumeration, uncertain action results and persistence are not promoted
to global PASS by this report.

## 5. Findings and disposition

Ten records are classified below. They are not ten confirmed security vulnerabilities.
[The findings register](R16_FINDINGS.json) includes preconditions, severity rationale,
confidence, affected SHA, evidence, owner, recommendation, correction identity and residual
risk. No invented CVSS score or exploitation claim is included.

| ID | Category | Finding | Disposition |
| --- | --- | --- | --- |

| R-001 | DOCUMENTATION_OVERCLAIM | Ordinary quickstart invoked initialization despite committed lockfile | REMEDIATED_FUNCTIONALLY_VERIFIED |

| R-002 | DOCUMENTATION_OVERCLAIM | Platform and architecture prose lagged the integrated implementation | REMEDIATED_REVALIDATION_PENDING |

| R-003 | DOCUMENTATION_OVERCLAIM | Current-commit evidence claim conflicted with historical and skipped jobs | REMEDIATED_REVALIDATION_PENDING |

| R-004 | DOCUMENTATION_OVERCLAIM | Contributor reporting instructions did not match available channels | REMEDIATED_FUNCTIONALLY_VERIFIED |

| R-005 | TEST_OR_EVIDENCE_DEFECT | R smoke selection includes zero-test libraries and checks exit codes only | OPEN_SCOPE_RESTRICTED |

| R-006 | UNCONFIRMED | Aggregate upstream tool-list budget requires further verification | UNCONFIRMED_OPEN |

| R-007 | TEST_OR_EVIDENCE_DEFECT | Independent eligibility and R-authored assurance changes are not adopted | BLOCKED_REVALIDATION |

| R-008 | TEST_OR_EVIDENCE_DEFECT | Requested review depth was not completed for all mandatory boundaries | BLOCKED_TOOL_ACCESS_AND_SCOPE |

| R-009 | TEST_OR_EVIDENCE_DEFECT | No main branch protection or ruleset was observed | OPEN_MAINTAINER_DECISION |

| R-010 | KNOWN_PLATFORM_LIMITATION | Physical and interactive platform release gates remain open | OPEN_KNOWN_LIMITATION |


The three review-closure blockers are **R-006, R-007 and R-008**. R-006 is provisional:
the visible federated tool-count check occurs after collection, but SDK aggregate bounds
and actual resource impact remain unverified. It is assigned for owner review, not presented
as a demonstrated vulnerability. R-007 requires separate adoption of R's own assurance
changes. R-008 records the incomplete direct-review coverage.

R-005 limits the interpretation of the smoke harness. Its exit-code checks passed; R then
inspected the actual log to verify the positive fake outcome. It cannot be used to claim
fresh Graph/Effects coverage or automatic assertion of that outcome. The blocked proposed
refinement is neither in the PR nor counted as a correction.

The lack of an observed main protection/ruleset is a maintainer governance decision, not
proof of an unauthorized write. R06/R18 and the interactive platform limits remain release
gates, separate from whether this report is complete in a narrower scope.

## 6. Implemented documentation and first-run path

README now has a concrete purpose, progressive navigation, a small broker/provider diagram,
a bounded first operation and clear implementation/native/interactive support distinctions.
Installation preserves the committed lockfile and no longer recommends initialization that
intentionally rejects it. Architecture now covers Linux/macOS/Windows, Provider Runtime,
Driver Host, Composition, Graph, Effects, Skills, recipes and lifecycle boundaries without
inventing a second authorization system.

Support, contribution and security guidance now identify the actual public issue/private
vulnerability-reporting channels. No SLA is promised. VERIFY and RELEASE_BLOCKERS now
separate current observations from the old failed preflight. The old failures, receipts,
owner reports and historical G instructions remain, with historical notices where edited.
No file or worktree belonging to another contributor was deleted or moved.

The entry example is a fake Export operation, not an export from Blender or Godot. From
the pinned checkout and prerequisites documented in the installation guide, the verified
commands are:

```sh
cargo build --locked -p semwright-daemon -p semwright-cli --bins
BIN_DIR=target/debug ./scripts/dev/fake-smoke.sh
```

The hosted execution reported fake=true, a completed recipe, changed=true and successful
nested invocation audit metadata. The example uses its own temporary runtime and the fake
backend. R did not certify every installer, package format, Nix path or interactive session
by running this example. The full release-build and uninstall variants retain their stated
historical evidence and limitations.

The inventory in [CLOSEOUT_STATUS.json](CLOSEOUT_STATUS.json) classifies 339 documentary or
procedural paths. Ten canonical public guides were updated; the broader changed documentary
surface includes historical notices and the review entry point. **212 inventory rows were
not individually reviewed** and remain UNKNOWN, not silently relabeled as current or obsolete.
Generated references, Skills, recipes, notices and useful history were preserved. The local
link checker verified targets in twelve named public documents; it does not validate all
external URLs or Markdown fragment anchors. This is not a global reference-documentation
certification.

## 7. Executed checks and historical evidence

| Evidence | Actual source/suite | Observed result | Important exclusion |
| --- | --- | --- | --- |
| R smoke, run 37099067854 / job 111134879759 | Product 6491c0d8; suite df173668 | Two-binary locked build, 21 library tests, fake changed=true | Graph/Effects --lib each ran zero tests; expected fake result inspected by R |
| R documentary push, run 37099590257 / job 111136349060 | Candidate 206ce247 | Validator, 10 documentary tests and hygiene passed | Positive-smoke job skipped on this later docs commit |
| Quality source/static jobs, run 37099614123 | Synthetic merge 21447095, tree identical to 206ce247 | 198 Python tests, 20 Node tests, static lints passed | Rust/MSRV skipped; some documentary tests overlap Python discovery |
| G run 37074787747 | Product cd518748; suite 73cf4806 | 480 case outcomes match summaries; 14 archive digests match | 105 are lab self-tests on the suite source; 348 contract and 27 native cases concern product |
| I acceptance and global disposition | Product cd518748; distinct suites | Original acceptance and failure/retest disposition retained | Not independent R16 approval; not H evaluation |
| Windows run 37096430846 | Product 6491c0d8 | Native x64/ARM64 and selected sealed-tool jobs passed | Not unlocked-desktop R18 acceptance |

The [fresh smoke record](evidence/R_FROZEN_SOURCE_SMOKE.json) retains the exact commands,
return codes, timings, log digest and lock/source unchanged checks. The
[documentary source record](evidence/R_DOCUMENTARY_SOURCE_CHECKS.json) retains every job
status and the synthetic-merge tree proof. The
[G receipt verification](evidence/G_RECEIPT_VERIFICATION.json) compares every case's outcome
with the summary and keeps the original source/suite/limitations. Archive checks establish
byte integrity, not independent correctness of every historical oracle.

The later PR dependency/fuzz/coverage workflow ran classification only. Its three heavy
jobs were skipped. Native integration also ran classification only; twelve application jobs
were skipped. This is appropriate selection for the documented diff but is not fresh native
or supply-chain certification. No mutation/fuzz/model benchmark or complete platform matrix
was rerun by R. Release admission still returns exit 2 for readiness/live/security gates;
its input and guard were not weakened.

## 8. PR and historical-work disposition

The current open set before R consisted of four dependency PRs. All remain open and untouched.
Their current changed paths and full head SHAs are retained in CLOSEOUT_STATUS. This mission
did not claim an untested version upgrade was equivalent to current source.

| PR | Current scope | R disposition |
| --- | --- | --- |
| #166 | hmac 0.12.1 -> 0.13.0; root/driver manifest and lock | Preserve open for dependency/compatibility review |
| #165 | getrandom 0.2.17 -> 0.4.3; driver manifest and lock | Preserve open for dependency/compatibility review |
| #161 | sha2 0.10.9 -> 0.11.0; manifest and lock | Preserve open for dependency/compatibility review |
| #157 | setup-node update across four workflows | Preserve open; do not assume already superseded from another workflow |
| #207 | R documentation, verification and review evidence | Own draft; no merge or release performed |

The captured API and ancestry checks confirm the merged relevant heads #168, #172-#176,
#183, #192, #198, #201 and #204 are ancestors of the review target. Closed #206's head is
also included. Closed #188, #197, #199, #200, #202 and #205 were **not proven superseded by
head ancestry**; their PR histories/objects/worktrees were preserved and no removal was
performed. No unknown unique contribution was discarded to make the repository look tidy.
This is a preservation disposition, not a claim that R independently reconstructed every
closed alternative's patch equivalence.

## 9. Selective editorial source and repository metadata

The user-selected github-optimization-skill was read at
`3f20ca72981f19c8e16b5aa0b453b96d682b458b`. README and SKILL hashes are in the manifest.
It was treated as editorial reference, not installed, executed as a dependency or
redistributed. All new Semwright wording is original to this closeout.

| External idea | Actual change | Excluded advice and reason |
| --- | --- | --- |
| Concrete purpose and progressive disclosure | New README introduction/navigation and concise architecture entry | Stars/conversion claims and branding are not evidence |
| Usable quickstart | Locked two-binary fake example and install/uninstall distinctions | No installer shortcut or unverified release-download path |
| Helpful links/support/contribution routes | Updated support, contribution, security and versioned evidence links | No automated repository metadata edits, contacts or publication |
| Editorial simplification | Source-scoped support and assurance language | No automatic license replacement or chained humanise-text installation |

A possible metadata description, not applied, is: “Policy-gated semantic commands and
verified workflows for desktop applications.” Possible topics are `rust`, `mcp`,
`desktop-automation`, `accessibility` and `application-integration`. The maintainer can
assess them separately; R did not change description, topics, homepage, visibility, licensing
or repository protection settings.

## 10. CI, resources and data handling

Only existing compatible standard hosted GitHub Actions jobs were used for compilation
and test execution. No large device builds, native application runtimes, package-manager
installs, cache construction, display capture or live application state manipulation ran
on the connected device. Device work was source/document editing, small Python validation,
Git/API metadata and small receipt/archive handling.

CircleCI had no authorized CLI/token or available connector in this session. R did not
search for credentials in private configuration, fabricate CI environment identity or claim
that CircleCI credits were checked. No paid larger runner, new cache/artifact upload,
billing setting or paid service was requested by the added workflow. This describes the
observed configuration; R did not audit the account invoice.

Private raw observations and logs are kept apart from the public delivery. The public
bundle excludes credentials, personal filesystem paths, .git, targets, toolchains, caches,
node_modules, real user application data and large renders. Historical sources were not
rewritten, deleted or converted from FAIL to PASS. Every referenced digest is labeled for
the file/archive it actually hashes.

## 11. Adoption, compatibility and evidence-only delivery

The source candidate changes nineteen paths: documentation/historical notices, one new
workflow, two R verification scripts, a small test module and the review entry point.
There are no product Rust, Cargo.lock, release-readiness, license or notice changes.
No compatibility namespace or generated capability/schema was edited. Existing roots,
providers, applications and user data need no migration from this documentary patch.

The later review commit adds only this evidence surface under `verification/r16-closeout`.
Its own commit hash is recorded externally after it exists, avoiding a self-referential
report. Tests of source candidate 206ce247 or merge 21447095 are not described as executions
on that delivery commit. The backup includes the actual changed documents and exact-base
patches, plus payload checksums and a separate archive SHA-256.

Review the PR and its scoped records together. A new runtime, quickstart behavior, workflow
permission, Skill or product correction after this freeze needs a new source identity and
impact-appropriate verification. Do not toggle security_review or merge/publish solely
because the documentary workflow passes.

## 12. Mission acceptance and decisions

The following matrix describes this mission, not the old release-blocker numbering.
PARTIAL and blocked rows are intentional. Overall full mission satisfaction is **not claimed**.

| ID | Status | Evidence / remaining condition |
| --- | --- | --- |

| CO01 | SATISFIED | Current main, worktrees, PR metadata and target snapshot captured. |

| CO02 | SATISFIED | I target admitted; all twelve boundaries mapped with distinct evidence identities. |

| CO03 | SATISFIED_WITH_UNCONFIRMED_ELIGIBILITY | AI-assisted role and lack of independent adoption are explicit. |

| CO04 | PARTIAL | Direct-review tool blocks and unexecuted native/supply-chain scope remain R-008. |

| CO05 | SATISFIED_IN_DECLARED_SCOPE | Fifteen scoped claims map to source/evidence and limitations. |

| CO06 | SATISFIED | Ten classified findings include owners, impact, status and residual risks. |

| CO07 | SATISFIED_WITH_REVALIDATION_BLOCK | Documentary corrections have functional checks; own assurance changes remain independently unvalidated. |

| CO08 | SATISFIED_WITH_PRESERVATION_DISPOSITION | Current PRs inventoried; merged heads checked; unproven closed alternatives preserved, not discarded. |

| CO09 | SATISFIED_FOR_UPDATED_GUIDES | Fourteen public/historical documentary paths and review entry point changed; inventory retains unknown rows. |

| CO10 | SATISFIED_FOR_FAKE_QUICKSTART | Real hosted two-binary build and fake recipe observed; other install variants not reexecuted. |

| CO11 | SATISFIED_FOR_CHANGED_SURFACE | No license/notices/history deletion; local links checked in twelve public documents. |

| CO12 | SATISFIED | Pinned external source read and selective adoption recorded; no installation or promotion. |

| CO13 | SATISFIED_IN_OBSERVED_CONFIGURATION | Selective standard hosted jobs, no device builds/new paid runner/cache/artifact upload or billing changes. |

| CO14 | SATISFIED | Review target, source candidate, suites and later report artifacts remain separate. |

| CO15 | FINALIZED_BY_EXTERNAL_DELIVERY_MANIFEST | PR207 exists; archive/reproducibility and final delivery commit are recorded outside the self-referential report commit. |

| CO16 | SATISFIED | R16/H/technical acceptance/merge/release remain separate; maintainer authority preserved. |


### Maintainer decisions requiring authority

Adopt a separate reviewer for R-authored assurance changes and resolve R-006/R-008 before
any R16 closure decision. Decide the repository protection/required-check policy and any
optional metadata edits. Decide whether and when to merge PR #207; dependency upgrades,
H budget, outstanding physical/interactive gates and release publication remain separate
choices. R has not performed or preapproved any of these actions.

The actionable next technical work is assigned in the findings register; the maintainer
is not asked to reconstruct the integration or manually rediscover every outdated README.
The present delivery is useful now as corrected entry documentation and a source-specific
review record, but it is not a complete independent security sign-off.
