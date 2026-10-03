# Composition / Figma / Motion / AV traceability

> Historical Composition/AV traceability checkpoint. Final combined engineering certificates and main-promotion policy are recorded in [the integration ledger](../semantic-creation/INTEGRATION.md); pending states below retain their original checkpoint scope.

This file maps the original Composition/AV requirement IDs to implementation and evidence. It is not an acceptance report. IMPLEMENTED means code/tests exist; only exact-SHA CI/native evidence may change a gate to PASS. Combined Composition+Audio rows remain PENDING until an audio-ready revision and one combined candidate SHA exist.

## Composition / AV requirement mapping

| ID | Implementation / evidence surface | Current state |
|---|---|---|
| A01 | Architecture/research delta, pinned-runtime notes, upstream boundaries | IMPLEMENTED; exact-SHA package gate pending |
| A02 | C0 generic Composition/media-time; C1 AV consumer and ArtifactHandoffHint | C0 consumed by Audio; C1 remains the AV consumer boundary; exact combined proof PENDING |
| A03 | PlanVault, Controller, profiles, evidence, budgets, Broker executor | IMPLEMENTED; exact-SHA CI pending |
| A04 | Eight Figma composition capabilities retained over common lifecycle | IMPLEMENTED; exact-SHA Figma native evidence pending |
| A05 | Server-owned plans, freshness and cumulative repair budgets | IMPLEMENTED; exact-SHA CI pending |
| A06 | Film/Sequence/Beat/Shot model above video-domain | IMPLEMENTED |
| A07 | Motion inspect/plan/apply/measure/validate/repair/verify and native jobs | IMPLEMENTED; exact-SHA native evidence pending |
| A08 | 40 typed primitives, 12 archetypes, versioned editorial system | IMPLEMENTED |
| A09 | Native layout, Unicode/RTL text, font evidence, semantic aspect reflow | IMPLEMENTED; exact-SHA native evidence pending |
| A10 | Temporal DAG, exact rational solver, hard/soft constraints | IMPLEMENTED |
| A11 | Renderer observations, explicit frame coverage, speed/acceleration | IMPLEMENTED; exact-SHA native evidence pending |
| A12 | Deterministic validation and bounded repair candidates | IMPLEMENTED; M07 hold/retime regressions added |
| A13 | Fixed AV graph, public audio receipt boundary, dependency reuse/invalidation | AV-SIDE IMPLEMENTED; previous audio-ready SHA superseded; df2654bed recertified PASS but fresh formal audio-ready revision and combined exact-SHA E2E PENDING |
| A14 | Motion render, FFV1 mezzanine, MLT H.264/AAC mux, post-encode audio decode/analysis, full decoded sync | IMPLEMENTED; AV-side backport passed CircleCI private exact-SHA iteration; final candidate certification pending |
| A15 | Figma, video and fail-closed AV Skills plus production runbook | IMPLEMENTED against public audio capabilities; exact combined Skill evidence PENDING |
| A16 | Contracts, Figma, Motion, AV, security, fuzz and mutation workflows | IMPLEMENTED; exact-SHA results pending |
| A17 | Reproducible technical benchmark harness | IMPLEMENTED; no advertising claim |
| A18 | Exact audio-ready revision and one combined candidate | PREVIOUS AUDIO CHECKPOINT SUPERSEDED; df2654bed full audio certification PASS observed, but fresh AUDIO_READY_FOR_INTEGRATION publication and one new combined candidate are PENDING |
| A19 | Code/docs/Skills/CI/package/runbook/evidence tooling | IMPLEMENTED except combined evidence/release package after A18 |

## General gates

| Gate | Mapping | Acceptance state |
|---|---|---|
| G01 | Frozen baselines, isolated worktrees, component status records | PENDING final combined provenance |
| G02 | audio recovery manifest/hashes | 8ed2d30 handoff superseded; B recertification PASS @ df2654bed6d2ac57d547846b69d16ea48b4a9ee3 / run 36942574098; formal handoff + combined evidence PENDING |
| G03 | Exact C0 shared; additive C1 handoff contract | C0 satisfied; C1 AV consumer boundary; combined proof PENDING |
| G04 | Common kernel in Figma/Motion and audio-authoring C0 consumer | PENDING public audio Composition + combined E2E |
| G05 | Plans/descriptors/Skills grant no authority; Broker rechecks | IMPLEMENTED; stale BeginPermit incarnation fix `e58886d50057e85c3fc9d3a14f1483de1c36270d` published; independent G-FIND-A-001 retest PASS 70/70 in run 36938854785 |
| G06 | Freshness/partial/unknown/cancel/budgets | IMPLEMENTED; revoke/expiry/cross-vault permit regressions added; exact-SHA G/native evidence pending |
| G07 | Inventory, mapping and evidence remain distinct | IMPLEMENTED |
| G08 | Heavy work hosted; no heavy generated outputs committed | IMPLEMENTED; final hygiene evidence pending |
| G09 | Composition+Audio integration on one SHA | Previous combined ancestry used a superseded audio checkpoint; current PR #204 rebuild awaits fresh B-ready publication before cutting and certifying the new combined SHA |
| G10 | Production runbook present; no promo video/R16 closure | IMPLEMENTED; final candidate prerequisites pending |

## Figma gates

| Gate | Mapping | Acceptance state |
|---|---|---|
| F01 | Existing wire/API compatibility and explicit migration | IMPLEMENTED; exact-SHA workflow pending |
| F02 | Native plugin layout/text/bindings under existing policy | IMPLEMENTED; native evidence pending |
| F03 | Recalculated digest cannot expand server-owned plan | IMPLEMENTED |
| F04 | Session/document/generation/ref freshness | IMPLEMENTED |
| F05 | Intentional overlays separated from prohibited overlap | IMPLEMENTED |
| F06 | Unobservable relations remain UNKNOWN | IMPLEMENTED |
| F07 | Repair/deny/ambiguous/cycle handling | IMPLEMENTED |
| F08 | Real Figma acceptance | PENDING exact-SHA native evidence; fixture CI is not a substitute |

## Motion gates

| Gate | Mapping | Acceptance state |
|---|---|---|
| M01 | Film -> managed binding/projection -> real render receipt | IMPLEMENTED; exact-SHA native evidence pending |
| M02 | 40 grammar realizations + 12 archetypes | IMPLEMENTED |
| M03 | Contradictory temporal constraints return cause | IMPLEMENTED |
| M04 | Native 16:9 / 9:16 / 1:1 replan-render-verify with stable IDs/cues | IMPLEMENTED; exact-SHA native evidence pending |
| M05 | Unicode/RTL/text/font/mask/overlay/clipping observations | IMPLEMENTED; exact-SHA native evidence pending |
| M06 | RangeCoverage records requested range, observed frames and exhaustive flag | IMPLEMENTED |
| M07 | Bounded repair/reflow/ExtendHold and downstream re-realization | IMPLEMENTED; hold slack/non-mutation regressions added |
| M08 | Authoring partial render uses frame-zero fallback; dependency invalidation tests | IMPLEMENTED conservative fallback |
| M09 | Native job cancel and request cancellation with sandbox process bounds | IMPLEMENTED; exact-SHA native evidence pending |
| M10 | FFV1 transfer + MLT encode + decoded media/sync | IMPLEMENTED; exact-SHA native evidence pending |

## AV gates

No AV row is PASS before the exact B-ready SHA is merged and the technical E2E executes on one combined candidate.

| Gate | A-side implementation | Acceptance state |
|---|---|---|
| AV01 | Exact Rate/Rational contracts; MLT fps + 48 kHz profile | PENDING combined |
| AV02 | Full-master flash/impulse decoder + pinned SyncSpec | PENDING combined |
| AV03 | Cue/timing dependency invalidates Motion+Audio dependents | CONTRACT TESTED; PENDING combined E2E |
| AV04 | Audio-only change retains Motion but rebuilds/reverifies master | CONTRACT TESTED; PENDING combined E2E |
| AV05 | Visual-only change preserves independent audio | CONTRACT TESTED; PENDING combined E2E |
| AV06 | Coordinator retains prior effects and refuses ready master after failure | IMPLEMENTED; PENDING combined failure E2E |
| AV07 | Owner/session binding on plans/artifacts/transfers | IMPLEMENTED; PENDING combined |
| AV08 | Mux re-probes duration/rate/channels/sample count; final audio must be analyzed again | A-SIDE IMPLEMENTED against public B analysis; PENDING combined E2E |
| AV09 | Motion + B common receipt + artifact.handoff + MLT | AV-SIDE CONNECTED; disposable E2E with corrected audio semantics PASS in run 36948655709; fresh formal audio-ready revision and final combined receipt/E2E PENDING |
| AV10 | Clean-runner package/runtime setup | PENDING combined clean run |
| AV11 | Cancellation/UNKNOWN semantics and provider cancellation | IMPLEMENTED; PENDING combined |
| AV12 | Dependency diff/reuse and new evidence after semantic change | CONTRACT TESTED; PENDING combined |

## CI discipline

| Gate | Mapping | Acceptance state |
|---|---|---|
| CI01 | Contracts/drivers/Figma/Motion/fuzz/mutation/native/package split | IMPLEMENTED |
| CI02 | Exact-SHA workflow dispatch plus run-ID evidence | IMPLEMENTED |
| CI03 | Test-count guards and nonempty mutation selector | IMPLEMENTED |
| CI04 | Runtime/package/artifact manifests and hashes | IMPLEMENTED |
| CI05 | Missing native prerequisite cannot become PASS | IMPLEMENTED |
| CI06 | No policy/security bypass added to make tests green | IMPLEMENTED; final review pending |
| CI07 | Full pertinent regression on one combined candidate | PENDING exact combined candidate certification |
| CI08 | Bounded logs/artifacts/caches and synthetic test data | IMPLEMENTED; final hygiene evidence pending |

## Evidence rule

A row may be changed to PASS only when the final evidence ledger names the exact tested SHA, workflow run, job IDs and retained artifacts/limitations. Earlier green SHAs are diagnostic only. The candidate-evidence gate rejects mixed-SHA evidence and rejects READY when B is not formally ready, required gates are not all PASS, R16 is closed, or a promotional video is claimed.
