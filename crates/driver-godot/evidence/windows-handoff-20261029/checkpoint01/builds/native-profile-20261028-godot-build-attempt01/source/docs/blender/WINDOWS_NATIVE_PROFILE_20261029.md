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
| P2: bounded composition | PASS on candidate09 | Two original meshes/materials, trusted Effect Conformance, independent readback, incremental transform, stale/replay rejection without effects, save/reopen and static structural preservation. Creative Archipelago production is the next step. |
| P3 / G6 | PARTIAL / NOT_COMPLETE | Mediated Godot integration, real project revisions and human-edit preservation remain pending. |
| P4 / art | NEXT / REVISE | Driver qualification permits the next typed creative asset; no new artistic acceptance is claimed. |
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

## Next closure

Produce substantial original Archipelago geometry through the qualified typed composition route, update the existing Godot project, preserve the human cameras and compare three hero shots against v03. The static asset caller accepts bounded mesh specifications; it exposes no arbitrary Blender Python.
