# Blender profile parity — evidence boundary, 2026-10-29

Status: **experimental, partial qualification, local review only**.

This matrix separates implemented runtime selection, component checks and physical
production-Host evidence. It does not certify the complete Blender surface or
close Archipelago A05/G6. The driver README and the latest sections of the
[current profile](WINDOWS_NATIVE_PROFILE_20261029.md) describe the present status;
earlier failures remain part of the evidence.

## Scope and interpretation

Reviewed source baseline: `0227529a8377739c5fb28c6e7ffcd92a58b8c849`,
branch `feat/blender-52-windows-isolated-driver`. This follow-up changes
documentation only.

Source inspected for this review includes
[`runtime_profile.rs`](../../crates/driver-blender/src/runtime_profile.rs),
the version/capability gates in
[`main.rs`](../../crates/driver-blender/src/main.rs),
the exact bridge version gate, selected semantic code, checkpoint test names,
and the native workflow. This is a bounded review, not a complete code audit.
No new Blender launch, build, native regression or component-test execution was
performed for this documentation follow-up.

Status vocabulary:

- **IMPLEMENTED**: the named contract is present in the inspected source. It does
  not imply a successful execution on every supported runtime.
- **TESTED_COMPONENT**: a named historical component-test report exists; the
  exact report/source identity limits the claim.
- **HOSTED_NATIVE_PASS**: a physical production-Host result on the exact recorded
  candidate, binary, runtime, configuration and fixture.
- **BLOCKED**: an inspected authority or compatibility contract prohibits the
  named operation in that profile.
- **NOT_RUN**: no qualifying execution for the stated profile/revision is
  established here.

Historical candidate results do not become new native qualification of the
current HEAD because their documentation was reviewed. Frozen manifests, build
reports, source overlays, requests, envelopes and receipts remain authoritative.

## Exact runtime profiles

| Profile | Runtime-selection contract | Native qualification for this contribution |
| --- | --- | --- |
| Linux Blender 4.5.14 | IMPLEMENTED; exact tuple, banner and Linux resource layout | NOT_RUN; retained workflow described below |
| Linux Blender 5.2.1 | IMPLEMENTED; exact tuple, banner and Linux resource layout | NOT_RUN; requires a separately pinned 5.2.1 execution configuration |
| Windows Blender 5.2.1 | IMPLEMENTED; exact tuple, banner and distinct Windows resource layout | HOSTED_NATIVE_PASS for the bounded candidates below |

The source rejects unsupported versions, mismatched banners and unsupported
platform/version combinations. The three selection profiles do not establish
deep-authoring parity. On 5.2.1, the capability gate allows RNA-tagged operations
only when their declared risk is ReadOnly; curated/Composition operations follow
their own registered contracts.

## Windows capability evidence

Every Linux native capability row corresponding to this table remains
**NOT_RUN for this contribution**.

| Capability | Windows 5.2.1 status | Evidence and limit |
| --- | --- | --- |
| Pinned confined native startup | HOSTED_NATIVE_PASS | Production Host, actual LPAC/Job attestation, one native process and explicit owner opt-in. |
| Typed collection/object creation, transforms and independent readback | HOSTED_NATIVE_PASS | Candidate03 and later bounded fixtures; not complete Blender API coverage. |
| Original static mesh composition and simple materials | HOSTED_NATIVE_PASS | Candidate09 two-mesh/two-material fixture; candidates10/11 original project assets; candidate12 valid apply after rejection. |
| Read-only RNA inventory | HOSTED_NATIVE_PASS for bounded inventory | Candidate07 completed the expanded factory-data inventory. Candidate06 exhausted its bound and remains incomplete. |
| Generic RNA mutation on 5.2.1 | BLOCKED by the current capability gate | Non-read-only RNA-tagged capabilities are restricted. Readable metadata does not authorize mutation. |
| Broader UV/attribute and general node authoring | NOT_RUN; RNA-tagged mutations also subject to the gate | Source surfaces alone do not establish profile-qualified execution. Accepted recovery node types do not prove arbitrary graph or socket editing. |
| Collection-scoped new-only GLB export | HOSTED_NATIVE_PASS | Independent bytes/hash/structure checks, output-overwrite and traversal denials in recorded fixtures. |
| New-only checkpoint, verified close/reap, fresh hash-bound reopen | HOSTED_NATIVE_PASS | Candidate09 and later; only the declared static scene-data subset. |
| Preservation of unmanaged factory content | HOSTED_NATIVE_PASS for the fixture | Structural comparison does not establish preservation of a real human edit. |
| Unsupported animated, linked, constraint, modifier or simulation state in recovery | BLOCKED by the recovery contract | `static_mesh_scene_v1` rejects unsupported content; no full-scene/process promise. |
| Undo-history restoration | NOT_RUN; outside the promised recovery contract | A saved scene-data copy does not restore the original process or undo history. |
| CPU render through the Windows hosted route | NOT_RUN | A documented Linux render lane and direct Godot renders do not qualify this route. |
| GPU render in the tested Windows profile | BLOCKED by absent GPU authority | This records the tested profile, not an assertion that future Windows GPU support is impossible. |
| Stale/replay, traversal and no-overwrite checks | HOSTED_NATIVE_PASS for named probes | Candidate09 and other retained boundary probes; not an exhaustive denial campaign. |
| Readiness cancellation and process reaping | HOSTED_NATIVE_PASS for recorded scenarios | Candidate04 cancellation and candidate09/later lifecycle; acknowledgement and observed exit are separate. |
| Plan-output publication budget | TESTED_COMPONENT and HOSTED_NATIVE_PASS | Candidate12 rejects oversized output before publication, then completes valid authoring in the same native session. |
| Mediated Godot reimport/save and human continuity | PARTIAL; reimport/save and independent reopen verified | Five native approvals and the bounded support-scene persistence proof completed. Human-edit preservation remains pending; the raw stale-camera response retains UNKNOWN with separate no-effect reconciliation. |

## Historical evidence anchors

Frozen evidence is under
[`crates/driver-blender/evidence/windows-native-profile-20261029`](../../crates/driver-blender/evidence/windows-native-profile-20261029/).

| Record | Recorded SHA-256 | Scope |
| --- | --- | --- |
| Candidate09 lifecycle receipt | `5502aa846937c004beccd9d4684157abbb3864a4a6b48adaca55f26b4670290a` | New-only save, close/reap, fresh hash-bound reopen and readback |
| Candidate09 authoring receipt | `ef8b99af8b9c5894f8bbe1687e8b9cc961ff5ad2ff31467f2cd6de01ae3a8ab0` | Composition, conformance, incremental transform, stale/replay checks and persistence |
| Candidate10 r22 receipt | `f540a2b0265d4df55359bbdc9d9d23be6ce5c6b7976f49fd5f66ab932cb33f5f` | Original three-mesh rock; 518 source vertices, 1,024 triangles |
| Candidate11 r23 receipt | `e3606a8eea0f2d06d0604eb278fc8109a658098a3414d9550a4ba4d9fb8e0c6f` | Separate eight-vertex, 12-triangle threshold and lifecycle/export |
| Candidate12 receipt | `86ce6ba88a9ba004690fcd5f15503a397a9c5a8b5b258e58f96fc91cbaa035e4` | Same-session rejection/recovery, persistence and GLB |

These anchors refer to existing records; this documentation follow-up does not
claim a new byte-for-byte verification of every archived artifact.
`checkpoint01` retains candidates03–07 and their failures;
`checkpoint02` retains candidate08 failure/candidate09 success;
`checkpoint03` retains creative candidates10/11 and earlier failed attempts;
`checkpoint04` retains candidate12 with 18 correlated request/envelope pairs.
See [plan-output details](PLAN_OUTPUT_BUDGET_20261029.md).

The current profile reports 19 checkpoint tests and three bundle tests from
earlier builds, then 20 driver and 60 registry/types tests for candidate12.
The existing [checkpoint test inventory](../../tests/python/test_blender_checkpoint.py)
covers aggregate-ID identity checks, unknown content, actual compositor trees,
empty sequencer shells versus content, static migrated nodes, runtime-image
state, orphan retention and source flags, advisory inspection, new-only save,
failed-save preservation and fixed fresh-open flags. Names were inspected here;
those historical tests were not rerun. Aggregate counts are not per-platform
qualification.

## Recovery and authority boundaries

`static_mesh_scene_v1` is a bounded recovery copy of supported scene data.
It rejects unsupported external/linked, animated, simulation, modifier,
constraint, compositor and meaningful image state. Acceptance of empty migrated
factory structures and exact static node types is a compatibility case, not
general sequencing or node-graph support.

Allowed orphan datablocks receive intentional retention treatment in the copy.
The source flags must be restored and verified before publication. New-only
publication, hash-bound fresh initial open, and legacy destructive-operation
consent remain distinct. There is no promise of undo, complete shader socket/UI
equality, full process restoration or replacement of a live edited session.

The Windows native profile remains disabled by default. Host owns the pinned
runtime, fixed bootstrap/arguments, bounded mounts, LPAC, attested one-process Job
and lifetime. Its explicit owner-authorized `registryRead` capability permits
Windows ACL-authorized reads; it is not a per-key whitelist. Native Blender has
no network/GPU grant, and its Rust driver has a separate zero-capability LPAC.
This configuration alone is not an exhaustive physical denial campaign.

The Godot driver is confined, but the paired owner's Godot Editor is a separate
owner process. No arbitrary Python/eval/operator escape hatch, owner-token
fallback, global ACL relaxation or automatic human approval is introduced.

## Reproducible Linux lane, not executed here

The existing
[`native-integrations.yml`](../../.github/workflows/native-integrations.yml)
contains the pinned **Linux 4.5.14** lane. It downloads the official archive,
checks its checksum, sets `SEMWRIGHT_TEST_BLENDER_ROOT`, and performs native
Bubblewrap and Landlock prerequisites. The following commands are extracted
from that workflow and are not standalone substitutes for those prerequisites:

```bash
cargo build --locked -p semwright-driver-blender \
  --bin semwright-blender-driver --bin semwright-blender-session-runner \
  -p semwright-plugin-host --bin semwright-sandbox \
  -p semwright-cli --bin semwright \
  -p semwright-daemon --bin semwrightd

SEMWRIGHT_TEST_BLENDER=1 \
SEMWRIGHT_TEST_SANDBOX_HELPER="$GITHUB_WORKSPACE/target/debug/semwright-sandbox" \
cargo test --locked -p semwright-driver-blender \
  real_blender_driver_introspects_rna_renders_and_saves_inside_sandbox \
  -- --ignored --nocapture

BIN_DIR=target/debug scripts/dev/blender-driver-smoke.sh
```

The workflow separately runs the interactive add-on through Xvfb:

```bash
BIN_DIR=target/debug BLENDER_BIN="$SEMWRIGHT_TEST_BLENDER_ROOT/blender" \
  scripts/dev/blender-addon-live-smoke.sh
```

That add-on lane is separate from the production deep-driver proof. The exact
5.2.1 runtime selector does not turn the 4.5.14 workflow into a pinned 5.2.1
qualification lane: its archive/checksum/environment and applicable expectations
must first be configured and recorded separately on a real Linux host.

No Linux execution or external CI dispatch is authorized or claimed by this
document. Both profiles remain NOT_RUN.

## Pre-submission Archipelago handoff boundary (historical)

Archipelago commit `b25d813` adds a live read-only guard around the existing
prepared Godot handoff. Its separate 16 component tests passed; the physical
preflight completed seven first-party broker reads with zero approval requests
and zero mutation submissions. The receipt is:

`E:\Projects\worldwright-archipelago\evidence\integration\20261029-godot-broker\godot-bridge01\operator-handoff-attempt01.preflight-20261029T043551Z-bba392a5\receipt.json`

Receipt SHA-256:
`bbebdb2ed3c84b56644ef9de813e0db5640cb141b727fe55e0a78bf431180055`.

The guard matched 3,094 source files against the prepared source/recovery
inventory, reobserved the active support fixture and camera, and observed both
unsaved-scene flags. The inactive main scene's live projection is explicitly
**NOT_REOBSERVED**. Its disk hashes are not a substitute for live human state.

This is readiness for an operator-assisted attempt, not completed reimport/save.
Submission must run a fresh preflight, and the owner must personally answer the
native one-use challenges. Independent persisted-scene reopen and human-edit
continuity remain unfulfilled. See
[the Godot handoff record](WINDOWS_GODOT_HANDOFF_20261029.md).

P3 remains PARTIAL, A05 remains PARTIAL_PENDING_PROJECT_CONFORMANCE, G6 remains
NOT_COMPLETE and A02 remains REVISE. No push, public PR, merge, release, global
installation replacement or external CI was performed by this follow-up.


## Subsequent native handoff and persistence

The owner completed five native challenges. The 35-pair proof passed; reimport/save responses were applied, the main scene's live projection and unsaved flag were preserved, and only the support scene changed (SHA-256 37017d169c9cec25511b99f28a48443bfc4d16c5dceffd4b1d56ff461bd8ef41). An independent native process verified the persisted four meshes, bounds and camera; its subsequent full inventory changed zero project sources.

The wrapper correctly remains UNKNOWN because the expected stale-camera Conflict is reported with outcome_known:false. Separate pinned-source/readback reconciliation supports an unchanged camera postcondition, without rewriting that response or replaying the mutation. Thus reimport/persistence has scoped physical evidence while real human-camera continuity remains NOT_DEMONSTRATED. This run used unchanged GLBs; geometric A-to-B resource updating is not claimed.

The [Godot handoff record](WINDOWS_GODOT_HANDOFF_20261029.md) contains exact receipt identities and checkpoint02: 137 payloads, 224 audit records, 47 correlated envelopes, 35 explicit pairs and zero unmatched requests. P3/A05 remain partial, G6 incomplete, art REVISE and both Linux native regressions NOT_RUN. No new Blender native test ran during this Godot follow-up.
