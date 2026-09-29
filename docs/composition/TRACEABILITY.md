# Composition / Figma / Motion / AV traceability

This file maps the Agent-A master requirements to implementation and evidence. It is not an acceptance report. IMPLEMENTED means code/tests exist; only exact-SHA CI/native evidence may change a gate to PASS. Combined A+B rows remain PENDING until B publishes its formal handoff and one combined candidate SHA exists.

## Agent A mission

| ID | Implementation / evidence surface | Current state |
|---|---|---|
| A01 | Architecture/research delta, pinned-runtime notes, upstream boundaries | IMPLEMENTED; exact-SHA package gate pending |
| A02 | C0 generic Composition/media-time; C1 AV consumer and ArtifactHandoffHint | C0 consumed by B; final C1 consumption PENDING B |
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
| A13 | Fixed AV graph, B public receipt boundary, dependency reuse/invalidation | A-SIDE IMPLEMENTED; real B/combined E2E PENDING |
| A14 | Motion render, FFV1 mezzanine, MLT H.264/AAC mux, full decoded sync | IMPLEMENTED; exact-SHA native evidence pending |
| A15 | Figma, video and fail-closed AV Skills plus production runbook | IMPLEMENTED; public audio Composition requirement PENDING B |
| A16 | Contracts, Figma, Motion, AV, security, fuzz and mutation workflows | IMPLEMENTED; exact-SHA results pending |
| A17 | Reproducible technical benchmark harness | IMPLEMENTED; no advertising claim |
| A18 | Exact B handoff and one combined candidate | BLOCKED BY B_READY=false |
| A19 | Code/docs/Skills/CI/package/runbook/evidence tooling | IMPLEMENTED except combined evidence/ZIP after A18 |

## General gates

| Gate | Mapping | Acceptance state |
|---|---|---|
| G01 | Frozen baselines, isolated worktrees, A.json/B.json | PENDING final combined provenance |
| G02 | B rescue manifest/hashes | PENDING B |
| G03 | Exact C0 shared; additive C1 handoff contract | C0 satisfied; C1 consumption PENDING B |
| G04 | Common kernel in Figma/Motion and B audio-authoring C0 consumer | PENDING public audio Composition + combined E2E |
| G05 | Plans/descriptors/Skills grant no authority; Broker rechecks | IMPLEMENTED; exact-SHA CI pending |
| G06 | Freshness/partial/unknown/cancel/budgets | IMPLEMENTED; exact-SHA CI/native pending |
| G07 | Inventory, mapping and evidence remain distinct | IMPLEMENTED |
| G08 | Heavy work hosted; no heavy generated outputs committed | IMPLEMENTED; final hygiene evidence pending |
| G09 | A+B integration on one SHA | PENDING B |
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
| AV03 | Cue/timing dependency invalidates Motion+Audio dependents | CONTRACT TESTED; PENDING B E2E |
| AV04 | Audio-only change retains Motion but rebuilds/reverifies master | CONTRACT TESTED; PENDING B E2E |
| AV05 | Visual-only change preserves independent audio | CONTRACT TESTED; PENDING B E2E |
| AV06 | Coordinator retains prior effects and refuses ready master after failure | IMPLEMENTED; PENDING combined failure E2E |
| AV07 | Owner/session binding on plans/artifacts/transfers | IMPLEMENTED; PENDING combined |
| AV08 | Mux re-probes duration/rate/channels/sample count; final audio must be analyzed again | A-SIDE IMPLEMENTED; PENDING B final-audio analysis |
| AV09 | Motion + B common receipt + artifact.handoff + MLT | A-SIDE CONNECTED; PENDING B real receipt/E2E |
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
| CI07 | Full pertinent regression on one combined candidate | PENDING B |
| CI08 | Bounded logs/artifacts/caches and synthetic test data | IMPLEMENTED; final hygiene evidence pending |

## Evidence rule

A row may be changed to PASS only when the final evidence ledger names the exact tested SHA, workflow run, job IDs and retained artifacts/limitations. Earlier green SHAs are diagnostic only. The candidate-evidence gate rejects mixed-SHA evidence and rejects READY when B is not formally ready, required gates are not all PASS, R16 is closed, or a promotional video is claimed.
