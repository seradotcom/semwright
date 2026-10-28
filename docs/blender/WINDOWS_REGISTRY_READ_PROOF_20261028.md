# Windows registryRead: authorized physical proof — 2026-10-28

## Result and scope

The owner authorized the bounded registryRead experiment and similar tests in this conversation; see WINDOWS_SXS_AUTHORIZATION_20261028.md.

A causal A–B–A comparison passed on LAPTOP-UDQOQQ3J:
1. Exact CPU PE resource 2 activation under LPAC with zero capabilities failed with Win32 5 / NT c0000022.
2. The same operation with exactly registryRead succeeded; queried activation identity matched the owner control.
3. A fresh zero-capability LPAC process failed again with the original denial.
4. The official pinned Blender 5.2.1 executable then ran --version successfully inside LPAC, exact Job, one process, no network.

A subsequent initial-process Blender test passed through the fixed first-party bridge:
- Native summary reported 5.2.1 LTS, 4,055 RNA types and 2,499 operators.
- Created IsolatedProof collection and IsolatedProofCube.
- Typed transform changed location to [3,4,5] and scale to [1,2,1].
- Independent typed object.get returned that state.
- Saved isolated-proof.blend and exported isolated-proof.glb.
- Parsed GLB 2.0: one node, one mesh, 1,784 bytes; expected Y-up translation [3,5,-4] and scale [1,1,2]; no external URI dependencies.
- GLB SHA256: edb4dd1d068cc77ccb95a0f7bf3546d85f00824423838bc7911a67210d8435ec.
- Native process exited 0 after input EOF, output drained; code and executable hashes remained unchanged.

The pre-ResumeThread Host receipts independently verified actual LPAC, owner and AppContainer identities, exact capability SID/attributes and exact Job. The registryRead SID hash was 66ad18b3326a9cb33bf2c59d57d4029ce756e0634c6d0fb3cfe09d6550a213f5 with attributes 4.

## Explicit limits

This was a test-only Host entry, not the broker production path. A05 remains BLOCKED. Godot import of this diagnostic GLB has not been run. Linux 4.5.14 remains NOT_RUN. There is no GUI, GPU, sustained-performance or artistic-acceptance claim.

The diagnostic bootstrap queried and checked actual bpy executable/resource locations, then set BLENDER_SYSTEM_RESOURCES inside the process for the existing bridge's validation. This does not demonstrate Host-provided startup environment. First-party module origins were checked against the read-only code directory. The production profile must build its own environment before process creation.

registryRead is a capability recognized by Windows ACLs, not a one-key SideBySide whitelist. Network was disabled, process limit was one, LPAC and Job controls were preserved, and no system registry/filesystem ACLs were changed. Only normal per-container grants on explicitly authorized test/runtime mounts were used.

## Implementation and validation

Feature windows-sxs-test exposes an explicit diagnostic function; normal sandbox_spawn always selects the original default mode. windows-launch-test-fixture forwards that feature for integration tests only. Production runner's zero-capability verifier is unchanged.

Completed checks:
- Host diagnostic guards: 5 passed (network, process limit, architecture, emulation, valid bounded request).
- Existing exact Blender manifest initial fixture: passed with zero capabilities.
- Registry fixture rejects owner token: passed.
- A–B–A plus actual Blender --version: passed.
- Initial Blender bridge typed edit/readback/save/export: passed.
- Production runner cargo check without diagnostic features: passed.

Preserved failed attempts:
- Host guard first compile: missing qualified SandboxKind, fixed before any process test.
- Initial bridge attempt 01: SDK rejected a non-root mount subpath before process creation. No SDK control was relaxed. Attempt 02 used the read-only code root as CWD and the fixed bootstrap.py filename.

## Evidence

crates/driver-blender/evidence/windows-activation-20261028/registry-read-authorized/INDEX.json lists SHA256 and sizes. It includes commands, actual outputs, failed attempts, A–B–A source snapshot and bridge proof artifacts. Raw execution folders are under .local/logs/registry-read-20261028-*.

Runtime pins:
- blender.exe: 284f4041f98e113f3dc10654a7193ffaaa9bfdfec8b87fa116620a48b5f6d4cb
- blender_cpu_check.dll: 1c1f06615a3fcca7c2757221fbafa3640e90591dbbc9d06f19f4962f5ce6194c

## Next work

Add an explicit owner-granted Windows Blender 5.2.1 initial-process profile to existing Host v8 sessions. Host owns the fixed first-party bootstrap, arguments, environment, registryRead request and nonce readiness. Driver remains limited to typed commands through the opaque session. Preserve Linux's existing runner and socket route. Prove broker mutation/readback/export and Godot import before closing A05.
