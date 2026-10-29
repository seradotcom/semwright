# Windows initial Blender profile — 2026-10-29 checkpoint

Status: **experimental WIP; local review only**. This checkpoint contains a real production Host path. It does not qualify the full Windows authoring surface or close Archipelago G6.

## Demonstrated and unresolved boundaries

| Criterion | Recorded result | Scope |
| --- | --- | --- |
| P0: initial native process | PASS on tested candidates | Host-owned Blender 5.2.1, LPAC, exact registryRead set, exact Job, one native process. |
| Explicit owner grant | PASS | False rejects before native preparation/launch. Disabled by default. |
| Broker typed scene operations | PASS | One official authenticated protocol Client; collection/object creation, transform and independent readback. |
| GLB and output boundaries | PASS | Independent bytes/hash/JSON/TRS; duplicate target and ../ traversal rejected; bytes/outside absence checked. |
| Native Godot consumer | PASS for candidate03 and candidate09 artifacts | Exact GLBs imported/rendered by Godot 4.7.2. Candidate09 checks two meshes, 12 triangles, AABBs, TRS, two materials and linear base colors. Godot broker was not used. |
| Cancellation during readiness | PASS on candidate04 | Correlated PID/nonce, real Cancel, Host-observed exit, no Ready/adoption; no injected native delay. |
| P1: complete persistence | PASS on candidate09 | New-only checkpoint, source flag restoration, confirmed close/reap, fresh hash-bound reopen and retained state; overwrite/live-replacement denials preserve the scene. |
| P2: bounded composition | PASS on candidate09 | Two original meshes/materials, trusted Effect Conformance, independent readback, incremental transform, stale/replay rejection without effects, save/reopen and static structural preservation. Candidates10/11 additionally produced original Archipelago rock and threshold components through the same typed route. |
| P3 / G6 | PARTIAL / NOT_COMPLETE | Real project revisions r22/r23 exist; mediated Godot integration and human-edit preservation remain pending. |
| P4 / art | IN_PROGRESS / REVISE | Original assets were made through the broker; the lighthouse mass remains conical and the overall scene remains far below the references. No human acceptance. |
| Linux 4.5.14 / 5.2.1 native | NOT_RUN | Existing route retained; Windows results cannot certify Linux. |

The original Semwright installation and interactive Blender were not replaced. Candidates use private local binaries, configuration, mounts, state and pipes. No upstream push, PR, merge, release or external CI was performed.

## Confinement contract

The first-party Driver Host owns the closed blender_5_2_1_initial profile. The driver supplies no native command line or bootstrap code. Host launch uses a pinned x64 executable, fixed arguments/environment, a fresh private runtime snapshot and compiled first-party resources.

Workspace is read/write without execute; runtime is read/execute; bootstrap code is read-only without execute. Source runtime files are checked for trusted ACLs, links/reparse points and physical identity, held against write/delete sharing while copied, individually hashed and inventoried. Source/workspace/state overlap is rejected. Staging is bounded and cancellable. The transient copy does not claim crash durability.

Launch remains suspended until actual token/Job attestation. Original process/Job controls remain held. Native resources request one process, no network and no GPU. The Rust driver has a separate zero-capability LPAC.

registryRead is an explicit owner-authorized capability required by the observed activation route. It permits Windows ACL-authorized registry reads; **it is not a SideBySide-only registry-key whitelist**. No network/GPU configuration is not an exhaustive physical denial campaign. See the earlier registry A-B-A proof and authorization record.

No third-party Blender MCP, arbitrary Python/eval/shell operation, owner-token runtime fallback, global registry ACL modification or automatic human consent answer is introduced.

## Recovery contract under implementation

Legacy destructive save/open/close operations retain their consent behavior. New typed operations provide a new-only recovery copy, read-only advisory inspection, save-and-verify before close, and a hash-checked initial open into a fresh native session. Initial open fixes use_scripts:false and load_ui:false and cannot replace a live edited session.

The static_mesh_scene_v1 contract rejects unsupported external, linked, animated, simulation, modifier, constraint, compositor and meaningful image state. Supported orphan datablocks receive temporary retention flags. Source flags are restored and verified before publication; intentional retention flags in the recovery copy are recorded. Undo history and exact whole-process restoration are not promised.

Publication is new-only and independently fingerprinted. Unknown mutations are not blindly replayed. Close acknowledgement and actual process exit require separate evidence.

## Preserved compatibility failures

Candidate03 completed broker operations/GLB, then failed checkpoint close. Factory data included empty VIEWER placeholders, a local orphan material and a palette. Recovery code now distinguishes empty nonpersistent placeholders from meaningful image state and retains allowed orphan data.

Candidate04 rejected all_ids. Blender 5.2 adds this read-only aggregate. Code now checks its metadata and exact identity-set coverage against individually inspected concrete roots, without duplicate orphan retention. Unknown nonempty categories still reject.

Candidate05 and candidate06 rejected sequencing/compositing/simulation. Scene.use_nodes returns True unconditionally since Blender 5.0; code now checks actual legacy/new compositor trees. Candidate06 read-only RNA diagnosis additionally observed an empty factory SequenceEditor and migrated material/light node trees. That diagnostic reached its 64-channel enumeration bound and remains failed/incomplete. Candidate07 completed the bounded read-only inventory: 129 channels with zero strips, empty meta stack and no active strip; migrated trees contain NodeFrame, Diffuse BSDF, Emission and Light Output. Build09 admits only the empty structural sequencer and these exact static node types, rejects frame text and animated/linked trees, and awaits the new physical lifecycle run.

Primary compatibility references:

- [Blender 5.2 Python API release notes](https://developer.blender.org/docs/release_notes/5.2/python_api/)
- [Blender 5.0 Python API release notes](https://developer.blender.org/docs/release_notes/5.0/python_api/)
- [Blender 5.2 Scene API](https://docs.blender.org/api/5.2/bpy.types.Scene.html)

## Packaging and validation

Host embeds seven exact vendored first-party resources. Its build script performs bounded read-only equality checks against all canonical sources in repository mode. Missing/drifting sources fail; standalone package mode still requires every vendored resource. Synchronization is an explicit developer command, never a build mutation. Python resources use LF.

Observed checks include 19 checkpoint unit tests, three bundle tests, Windows SDK/Host/runtime/configuration contract tests and driver unit tests recorded by individual build reports. Cargo package listing included the resources; this is not registry-install qualification.

Native candidates have binary pins/source snapshots, authenticated request/envelope records, Host PID/nonce attestations and preserved failures. Acceptance must use the exact tested candidate, not inherit results from another build.

Candidate08 reached the new inspection response but the closed driver output schema rejected the two added inventory counts. It remains a failed fixture, with no checkpoint save. The schema now declares those exact bounded counts; 17 driver tests and build10 passed. Candidate09 subsequently passed the full physical lifecycle fixture on build10.

Frozen evidence for retired candidates03–07 is under crates/driver-blender/evidence/windows-native-profile-20261029/checkpoint01. It preserves raw payloads, original failures, base-plus-overlay source snapshots and local audit/request/envelope correlation. See its manifest and README for scope.

## Physical closure on candidate09

Full lifecycle fixture: PASS, receipt SHA-256 5502aa846937c004beccd9d4684157abbb3864a4a6b48adaca55f26b4670290a.
Typed authoring fixture: PASS, receipt SHA-256 ef8b99af8b9c5894f8bbe1687e8b9cc961ff5ad2ff31467f2cd6de01ae3a8ab0.
Its recovery .blend is 660,355 bytes, SHA-256 683d2e2f2f5e944df3902bdeac96a30eb30cb821d9ba0d2e8a4dd45ba7098383.
The exported GLB is 3,232 bytes, SHA-256 4662b0d93ef7e6187c79e5d83829f8ed5dc75ef4153ab8080f021480ac672b52.
Native Godot 1280x720 capture SHA-256 f25155383f6719aa819ee7266c28f06248ee6b0eda2f8544731744c80b78da51.

The authoring proof compares fresh-session structure by RNA relation names/types and channel names/counts; it does not promise full shader socket/UI state equality. The content preserved outside the managed composition is a factory fixture, not a human edit. Expected Host termination follows verified saving; observed native exit code 1 accompanies kill_completed/wait_completed, not a claim of graceful Blender shutdown.

Frozen candidate08 failure and candidate09 success are in checkpoint02 beside checkpoint01. Source overlays identify their base commits in the build reports. The archive correlates 156 request/envelope pairs with local audit records. Published-feature readiness, Linux native regression, whole-product G6 and artistic acceptance remain open.

## Creative project revisions and next closure

Candidate10/static-asset-attempt03 produced the original r22 lighthouse rock: 518 source vertices, 1,024 triangles, three meshes and one material. Receipt f540a2b0265d4df55359bbdc9d9d23be6ce5c6b7976f49fd5f66ab932cb33f5f; GLB 6bdb95f2ecca8fd7528bc0b62240b975e152dd1e0ed0b8c3411289da6b1a78b3. Two prior attempts exceeded unchanged structural budgets; both failures are retained. The successful plan contained 12,751 JSON nodes.

Native Godot consumed the exact artifact in the existing project. A 16-point forward route passed, but an added 15-point return/three-lane route found a real 0.325 m deck lip and failed. Candidate11/static-asset-attempt01 then produced a separate eight-vertex, 12-triangle threshold via typed composition, persistence, fresh reopen and export. Receipt e3606a8eea0f2d06d0604eb278fc8109a658098a3414d9550a4ba4d9fb8e0c6f; GLB def7605cc8c5dc5e7a5e5a8e15d0741f61c69e07f5b92adb31360bb1493dceb8. The unchanged route and controller passed all 15 points after this geometric repair.

Four existing 1920x1080 views were captured in each revision. Art remains REVISE. No human camera-plan file exists, so unchanged agent camera definitions are only a technical preservation check. Godot was still a direct native consumer, not broker-mediated. Candidates10/11 were retired with every recorded PID absent.

Next: qualify the existing first-party Godot Windows Host pipe plus trusted EditorPlugin for bounded reimport and readback. The Godot driver compiled successfully; compilation is not execution or platform certification. Its editor process must be described separately from the confined driver. Full Windows authoring, real human-edit preservation and Linux regression remain open.

## Candidate12 — plan output publication regression

The exact PlanOutput now passes the unchanged shared registry value budget, typed output schema and final cancellation gate before any vault/record publication. Root and repair planning share the helper. Three publication regressions plus the existing driver tests passed (20 total); registry/types passed 60 tests.

Candidate12 physically rejected the original 1,062-vertex recipe at the driver-output preflight, preserved its native scene/session, and successfully planned/applied/read back a two-mesh fixture in the same isolated session. Library/checkpoint/reopen/GLB and Host reaping passed afterward. The initial connection-before-readiness failure remains preserved. checkpoint04 contains 18 correlated request/envelope pairs and the exact build/source records. Full detail: PLAN_OUTPUT_BUDGET_20261029.md.

## Godot broker — real partial progress

The first-party paired Godot driver now has physical doctor/session, wrong-key handshake rejection, instantiation and complete four-mesh readback evidence. The Editor is an owner process; its driver is LPAC with zero additional capabilities and a read-only project mount. Reimport and scene open require the broker's native human console. The corrected finalizer is compiled and prepared, not executed. A05/G6 remain incomplete, artistic acceptance remains independent, and Linux native regression remains NOT_RUN. Full detail and preserved failures: WINDOWS_GODOT_HANDOFF_20261029.md.
