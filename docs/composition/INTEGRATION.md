# Composition / Audio / AV integration record

This file is the source-controlled integration ledger. Fields marked PENDING must be replaced by exact evidence when B declares AUDIO_READY_FOR_INTEGRATION and the combined branch is created.

## Source lineage

- A frozen baseline: be375a12e8afa4d779f9dc0de501b0d4a262a682
- Common C0: 26602e4b25929be869d69ef28fef4dd9713180d7
- A branch: feat/composition-media
- B baseline: 93f70241e9fb9d4c99ca76fab55c8517574a9f6f
- B branch: feat/audio-completion
- B consumed C0 through normal Git ancestry: verified locally
- B observed working head at this checkpoint: ea144de5af1b663ff5ad821793c105b95f5f819d
- B AUDIO_READY_FOR_INTEGRATION: false
- Current origin/main reconciled into A: e3713e90e87f1caa8f7105c065094d5c724d144e
- Integration branch/worktree: PENDING until B-ready handoff
- Combined candidate SHA: PENDING

A and B remain separate until the B readiness handshake. A never edits the B worktree. The combined candidate will be created in the A-owned integration worktree, not in the canonical checkout.

## Contract history

C0 provides generic plan/base/evidence/lifecycle and exact media-time primitives. B has C0 as an ancestor rather than a duplicate private copy.

A later additive AV consumer contract defines media artifact metadata, audio consumer receipts, service proofs, staged coordination, final decoded sync and manifest publication. B is not considered to have consumed that additive contract until it publishes the exact consumed SHA/tests.

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
8. Record every run/job/artifact digest in this file and DEMO_PRODUCTION_HANDOFF.md.
9. Reconcile with current main once deliberately if needed; any code change creates a new candidate SHA and invalidates affected evidence.
10. Leave a reviewable PR/candidate. Do not close R16 or publish a release/video from this mission.

## Conflict policy

Expected shared conflicts are workspace dependencies, Cargo.lock, Skill inventory/docs and aggregate CI. Preserve all independently required packages/targets. A conflict is not resolved by taking one side's complete lockfile or deleting another agent's target.

Unexpected conflicts inside audio-domain/Faust/Ardour are returned to B for explanation. Unexpected conflicts inside Figma/Motion/common contracts remain A-owned.

## Candidate evidence

- A_HEAD_SHA: PENDING_FINAL_A
- B_READY_SHA: PENDING
- C0_SHA: 26602e4b25929be869d69ef28fef4dd9713180d7
- C1_CONSUMED_SHA: PENDING
- INTEGRATION_CANDIDATE_SHA: PENDING
- COMMON_CONTRACTS: PENDING
- FIGMA_NATIVE: PENDING
- MOTION_NATIVE: PENDING
- AUDIO_NATIVE: PENDING
- AV_NATIVE_MUX_DECODE_SYNC: PENDING
- SECURITY_TARGETED: PENDING
- REQUIRED_REPO_CHECKS: PENDING
