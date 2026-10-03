# Composition / Audio / AV integration record

> Historical Composition/Audio checkpoint. Final combined engineering certificates and main-promotion policy are recorded in [the integration ledger](../semantic-creation/INTEGRATION.md); pending states below retain their original checkpoint scope.

Historical Composition + Audio integration record. Final integration and runtime work now follow
[the semantic-creation ledger](../semantic-creation/INTEGRATION.md). The audio `8ed2d30` pin below
was superseded by `df2654be`; retain this section as checkpoint history, not current readiness evidence.

This file is a historical source-controlled integration ledger. Fields marked PENDING describe the recorded checkpoint; readiness requires an AUDIO_READY_FOR_INTEGRATION revision and one formal combined candidate tested on an exact SHA.

## Source lineage

- Composition frozen baseline: be375a12e8afa4d779f9dc0de501b0d4a262a682
- Common C0: 26602e4b25929be869d69ef28fef4dd9713180d7
- Composition branch: feat/composition-media
- Audio baseline: 93f70241e9fb9d4c99ca76fab55c8517574a9f6f
- Audio branch: feat/audio-completion
- Audio consumed C0 through normal Git ancestry: verified locally
- Previous audio formal handoff: 8ed2d30c8ba797ebd5b8c102d34f8ab5bb3a28b3; superseded after the formal combined E2E exposed a bounded-loudness receipt defect
- Audio PR/worktree head observed at this checkpoint: df2654bed6d2ac57d547846b69d16ea48b4a9ee3
- Audio exact-SHA recertification: run 36942574098 PASS with audio-gate PASS and retained final certification artifact; the new AUDIO_READY_FOR_INTEGRATION handoff is still PENDING because B.json has not yet published df2654bed as ready
- Current origin/main reconciled into Composition: e3713e90e87f1caa8f7105c065094d5c724d144e
- Integration branch/worktree exists at integration/composition-av for private diagnostics.
- Latest observed diagnostic integration head: 5648dd2da719b4602126f3458c3c266679c67b2a
- Formal combined candidate: integration/composition-av-formal; the exact commit is bound externally by candidate_sha and CANDIDATE_EVIDENCE so the source tree never self-certifies

PR #183 and native evidence identify recertified audio head df2654bed6d2ac57d547846b69d16ea48b4a9ee3, while the recorded `B.json` checkpoint still marks RECERTIFICATION_PENDING and audio_ready_for_integration=false. That checkpoint therefore exposes no audio-ready SHA for the combined candidate. Historical/private integration candidates remain diagnostic history only; a formal candidate must use a published audio-ready revision and obtain fresh exact-SHA certification.

## Contract history

C0 provides generic plan/base/evidence/lifecycle and exact media-time primitives. The audio line consumes C0 through normal ancestry rather than a duplicate private copy.

Effect-conformance E0 at `dd6d22d6ec6c7c5ef378da58ed75ca18b25ba5ff` was reviewed against the Composition ownership/authority boundary: effect conformance consumes Composition types and evidence, while PlanVault, canonicalization, lifecycle aggregation and Broker/policy authority remain runtime-owned. The approval is recorded on PR #172 and closes only the corresponding review dependency. The remaining integration gate requires `crates/effect-conformance` through normal workspace ancestry rather than a private worktree/path dependency. PR #183 records the requirement for the production audio consumer to publish the exact effect-conformance SHA it consumes before assembly of the formal combined candidate.

The additive AV consumer contract defines media artifact metadata, audio consumer receipts, service proofs, staged coordination, final decoded sync and manifest publication. The audio subsystem remains a provider of public audio capabilities; the AV layer owns the consumer boundary and must prove it on the exact combined candidate rather than duplicating shared C1 types in the audio implementation.

Wire-semantic changes after C0 must be listed here with migration/consumer tests rather than silently edited in both branches.

## CI execution policy

Iteration CI follows only the newest commit delta (`HEAD^..HEAD`) through `scripts/composition/ci-scope.py`. A change in one area runs only its dependent lanes: contracts, Broker bridge, packaging, Skills, Motion, Figma, MLT, fuzz or mutation as applicable. PR workflows that historically see the whole cumulative diff use the same scope helper so unrelated historical changes do not restart expensive native jobs. Composition fuzz/mutation keep lightweight PR checks but reserve heavy duplicate work for the branch push.

Full certification is separate. For the exact combined candidate SHA, dispatch the relevant workflows with `certify_all=true` and `candidate_sha=<40-char SHA>`. Each workflow fails before heavy work when the checked-out SHA differs. The final evidence manifest accepts only workflow/job evidence whose `tested_sha` equals that same combined candidate SHA. Previous green iteration runs are diagnostic history, not final PASS evidence.

The full native integration workflow preserves its historical all-backend jobs for certification and non-Composition branches. On `feat/composition-media` iteration, only affected Figma/MLT/driver-common jobs execute; the MLT iteration lane is separated from the combined KiCad+MLT certification job.

## Final integration procedure

1. Verify B.json/PR/branch all name the same AUDIO_READY_FOR_INTEGRATION SHA and required native audio jobs.
2. Verify the announced B SHA descends from C0 and its worktree is clean.
3. Create/update integration/composition-av in the dedicated A-owned worktree from the chosen current-main reconciliation point.
4. Merge A by normal Git ancestry.
5. Merge the exact B-ready SHA by normal Git ancestry. Resolve Cargo.toml/Cargo.lock/CI/Skills centrally; do not replace the newer lockfile wholesale.
6. Resolve any C0/C1 overlap structurally and run consumer/schema tests before native jobs. Bind B's verified final-audio artifact receipt to the owner-configured Broker source root/path separately from its provider token; configure the artifact-handoff destination root and the MLT `media` alias to the same delivery storage. Never derive a path from `MediaArtifact.reference`.
7. Execute the combined candidate gates on one SHA: common contracts, Figma regression, Motion native, audio native, real `artifact.handoff` audio transfer, AV MLT/decode/sync, targeted security and relevant required repository checks.
8. Record every run/job/artifact digest in the integration evidence for the exact candidate.
9. Reconcile with current main once deliberately if needed; any code change creates a new candidate SHA and invalidates affected evidence.
10. Leave a reviewable PR/candidate. Do not infer R16 closure or release/video publication from this historical integration procedure.

## Conflict policy

Expected shared conflicts are workspace dependencies, Cargo.lock, Skill inventory/docs and aggregate CI. Preserve all independently required packages/targets. A conflict is not resolved by taking one side's complete lockfile or deleting another integration target.

Unexpected conflicts inside audio-domain/Faust/Ardour are returned to B for explanation. Unexpected conflicts inside Figma/Motion/common contracts remain A-owned.

## Candidate evidence

- A_HEAD_SHA: 65b773f4dd627b860358342f4d40a1ac532566d1
- A_AFFECTED_DIAGNOSTIC: Composition diagnostics run 36960368022 PASS (Linux/Windows/macOS contracts + Broker session; Skills/package correctly out of scope)
- PREVIOUS_B_READY_SHA_SUPERSEDED: 8ed2d30c8ba797ebd5b8c102d34f8ab5bb3a28b3
- B_RECERTIFIED_SHA_AWAITING_FORMAL_HANDOFF: df2654bed6d2ac57d547846b69d16ea48b4a9ee3 (run 36942574098 PASS)
- B_READY_SHA: PENDING
- C0_SHA: 26602e4b25929be869d69ef28fef4dd9713180d7
- C1_CONSUMER_BOUNDARY: A-owned; exact combined E2E pending
- INTEGRATION_CANDIDATE_SHA: PENDING
- COMMON_CONTRACTS: PENDING
- FIGMA_NATIVE: PENDING
- MOTION_NATIVE: PENDING
- AUDIO_NATIVE: PENDING
- AV_NATIVE_MUX_DECODE_SYNC: PENDING
- SECURITY_TARGETED: PENDING
- REQUIRED_REPO_CHECKS: PENDING
