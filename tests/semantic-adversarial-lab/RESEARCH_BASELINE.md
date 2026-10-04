# Research baseline — 2026-09-29 UTC / September 28 Mexico City

Sources inform test design, not acceptance. Exact identities are in the target lock.

Read the Composition contracts in `docs/composition/{CONTRACT_HANDOFF,AUDIO_AV_CONTRACT}.md`, `crates/semantic-composition/src/{model,vault,controller,canonical}.rs`, `crates/av-composition/src/sync.rs`, manifests and diagnostics. Canonical integrity is not private vault authority or Broker policy, and it does not imply integrated audio readiness.

Read `crates/audio-domain/src/{lib,units,signal_analysis,wav}.rs` and diagnostics. PCM statistics are not LUFS/intelligibility; preserve undefined silence and distinguish sample peak from true peak.

Read baseline `docs/{architecture,permissions,drivers,skills}.md`, `docs/blender/SECURITY.md`, SDK, Skills public API, workflow inventories and native pins. Installation/metadata never grants authority. Native availability/acceptance remains to be tested against exact runtime pins, not inferred from docs/current.

Reuse baseline `scripts/dev/ci-driver-bwrap-profile.sh` as scoped runner provisioning, then independently prove canary/namespace isolation. Do not disable global AppArmor to produce green.

Official Actions documentation consulted:
- https://docs.github.com/en/actions/how-tos/manage-workflow-runs/re-run-workflows-and-jobs
- https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows
- https://docs.github.com/en/actions/reference/security/secure-use

Official commits verified through GitHub's API: checkout `3d3c42e5aac5ba805825da76410c181273ba90b1`; upload-artifact `043fb46d1a93c77aae656e7c1c64a875d1fc6a0a`; rust-toolchain `02cb101ec7c40f2c49e1d9714d64511d8e1b74de`. Rust 1.98.1 is the consumed contract baseline. Native sources and the full graph/effect consumer matrix still require explicit research and executed evidence before acceptance claims.

Continuation sources (2026-09-29 UTC): GitHub REST Actions artifact metadata/download and workflow-job attempt APIs inform the bounded collector; the force-cancel endpoint documents termination of an obsolete run whose empty `always()` gate survives ordinary cancellation.
- https://docs.github.com/en/rest/actions/artifacts
- https://docs.github.com/en/rest/actions/workflow-jobs
- https://docs.github.com/en/rest/actions/workflow-runs#force-cancel-a-workflow-run
These sources define the experiment design only. Native acceptance and source-audit claims still require executed evidence against the frozen targets.
