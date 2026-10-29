# Windows first-party Godot handoff — partial physical qualification

The paired first-party Godot route is physically operational on the pinned Windows laptop. The Godot driver runs under the production Driver Host in LPAC with zero added capabilities, no network permission and a read-only/no-execute project mount. The trusted project Editor is a separate owner process. The driver reports integration-under-test; this is not full platform certification.

Archive: crates/driver-godot/evidence/windows-handoff-20261029/checkpoint01.
Manifest SHA256: 17b9a7a6a649649acb26a65fd4c6356c224458203784c26e4e350b535c18dc5b.
There are 252 payloads / 3,698,928 bytes, a consistent 116-record local audit snapshot, 56 correlated envelopes and 37 explicit request/envelope pairs with zero unmatched explicit requests. The audit is a snapshot of an idle live broker, not a final sealed session or same-owner tamper resistance.

## Verified scope

- Godot 4.7.2-stable (official), project identity and first-party doctor/session route.
- A schema-valid wrong-key peer received the real challenge and was disconnected before Ready. It sent no operation or resume. The valid Editor session/generation was unchanged around this negative test. No explicit remote error reason is claimed.
- Broker-instantiated original Blender r22/r23 GLBs produced four MeshInstance3D nodes in an owned supporting scene.
- Curated native vertex readback was complete/global/base_mesh_vertices. Mesh identities, surface counts and AABBs matched the independently parsed GLBs within 0.0002 metres.
- Fixture camera properties and observed transform remained equal.
- Read-only reconciliation and a broker reconnection preserved the active fixture projection, its fingerprint, the fixture camera and both unsaved scenes. All 3,094 inventoried source files remained byte-identical.

Pairing receipt: 93da3d6d144df3b9dd3539f3c1f57c64257f12f66fba07289ca59a8fc43fe0ce.
Godot executable: c8f0a6bc45a19b33541501e57f6f7cd972ab18453743266339d495cbbe846643.
Godot driver: e3bd5d7a0ff1a83c7125881a113aba1a351024fa27f9ec4ddacca2bbe956dd33.
The 31 official plugin companion files were hash-checked before installation and after reconnection.

## Preserved failed and incomplete attempts

The first handoff stopped on the preexisting unsaved main scene before mutation. A later handoff created the supporting root/camera, then asset.reimport returned ConsentRequired before execution. The broker requires a native trusted-operator console for this CodeExecution capability; the owner chat authorization does not synthesize that input.

The existing-import continuation instantiated/read back the four meshes, then scene.save(save_external_resources=false) returned Unsupported. That conservative mode rejects nested scene instances before packing/saving. Its envelope retains outcome_known=false. The separate read-only reconciliation observed the same stamp and camera, an unsaved fixture, and the original root-only .tscn bytes. The enclosing test remains failed.

The first finalizer compared a session-scoped native reference as part of the camera data and stopped after three reads, before mutation. Its source inventory was unchanged. The corrected test excludes only the native ref from the compared node projection and retains raw envelopes.

project.godot normalization during Editor startup is recorded separately: effective renderer/filter values were checked natively. No byte-preservation claim spans that startup normalization.

## Native console and exact secret-lease recovery

The initial broker was replaced by the same pinned broker with --approval-console, preserving the live Editor. The first restart was rejected by private-file validation because pairing.secret retained two explicit ACL entries belonging to the retired driver AppContainer.

The repair matched the binary SID hash to the Host attestation, verified the retired process identities absent, checked the exact two ACE masks/types/flags, and removed only those expired entries. It preserved the secret bytes, owner, inherited rules and all other entries, with a concurrent-change check immediately before the write. No permissions were added; no private-file validator, policy or capability grant was changed.

The same candidate/configuration/key then started successfully in its native console. The existing Editor reconnected without plugin reload. Its newly acquired session and refs were used for post-reconnection reads. This is an observed recovery from forced retirement, not a general crash-cleanup guarantee.

## Prepared, not submitted

The compiled windows_broker_godot_finalize_known_fixture proof is prepared against the exact live fixture. Its remaining native confirmations are two pinned GLB reimports and three scene-tab opens. Each requires the operator's one-use console challenge; no approval API or automatic input is used. The native 120-second challenge expiry remains unchanged.

The test then checks fresh geometry/camera readback, saves the owned fixture using the existing supported external-resource mode, rejects a stale camera operation, and verifies the main scene projection and unsaved flag. A verified recovery copy of source bytes is retained. Permitted source changes are the supporting .tscn and the two explicit GLB import sidecars; source GLBs stay pinned. The .godot directory is explicitly excluded as cache/editor state.

Prepared test source: 7d89721ca05c3c3ce659c435fb589efa76e2f36f8f4364ae37cd5f685cfbbd8c.
Prepared test binary: c71d1bb5dcea20c482f3f5f794c6c9f89419f09707551c9b19348842c0ef75a6.
Prepared config: b353bdbc9bba4315445363682825efdac60f9a1c4ad6d68c8a3ec0b78297f982.
No PASS is attributed to this pending sequence. An independent fresh-process Godot readback of the saved fixture is also prepared for after its success.

## Boundaries

P3 remains partial, A05 remains PARTIAL_PENDING_PROJECT_CONFORMANCE, and G6 is NOT_COMPLETE. There is no demonstrated human-authored camera preservation and no geometric A-to-B reimport transition claim. Linux 4.5.14 remains NOT_RUN. Archipelago art remains REVISE and external publication remains unauthorized.
