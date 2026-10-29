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


## Completed native operator attempt and separate reconciliation

The owner personally completed all five native challenges in operator-handoff-attempt01. The original compiled proof completed 35 correlated request/envelope pairs and passed in 142.52 seconds. Both r22/r23 reimports and all three scene opens have allow_after_confirmation plus applied:true; the supporting-scene save has allow plus applied:true. Main's live node projection and its unsaved flag were preserved, and main was restored as the active tab.

Only proofs/semwright_godot_bridge01_02.tscn changed among 3,094 project sources: 118 to 736 bytes, SHA-256 37017d169c9cec25511b99f28a48443bfc4d16c5dceffd4b1d56ff461bd8ef41. Both GLBs and their import sidecars remained identical. The existing broker/editor stayed alive.

The enclosing guard correctly retains outcome UNKNOWN and passed:false. Its one uncertain response is the intentional stale-camera Conflict, request 7ededffa579241e2b0f794653c379725: the Host redacts driver errors and reports outcome_known:false. The original response and runner receipt remain unchanged. No mutation was replayed and the classifier was not weakened.

Separate read-only reconciliation passed. Camera projections before/after the probe matched excluding only ephemeral ref: FOV 60, revision 11 and identical fingerprint; the stale request proposed FOV 75 with expected revision 10. The installed plugin/rendering source hashes match the launch manifest. Their inspected control flow checks the expected revision before any camera write and returns that error synchronously without scheduling a mutation. This supports a scoped precondition-rejection inference and an observed unchanged postcondition; it does not rewrite the broker's original uncertainty.

A fresh native Godot process independently loaded the persisted supporting scene in 1.5 seconds. Four meshes, their surfaces and global bounds (tolerance 0.0002 m), plus ProofCamera settings and transform matched. A subsequent full source inventory found zero source changes from this observer. The observation was direct native Godot; the preceding reimport/save operations were mediated by the first-party broker.

Evidence anchors:
- Original proof receipt: d29825f0c29acf3dd48d564a70b8dc2e1c22d22e9fc0eb979df87ef9a89e12bc.
- Original UNKNOWN runner: 7f3b8401188bb1fffc4bc530c1e2b271862a211ab192c068c6723696a24da674.
- Separate reconciliation: ceca07a49625dd9ee0a1a8e5e028cd441a6680164cc126d85623756c0cb8510e.
- Independent persistence receipt: 3b94d195e0719e05ba48bd32606bd159f5eb2216563738a29920a07708839a72.
- Archive checkpoint02 manifest: d5fdb3556922795051f5e7243c99ffbb4704c58ee8579c9ba7a47e7e4f596ef8.

checkpoint02 contains 137 original payloads / 1,676,522 bytes, a consistent 224-record live-idle audit snapshot, 47 audit-correlated envelopes and 35 explicit request/envelope pairs with zero unmatched requests. Original build records remain in checkpoint01. Credential material was checked in memory and excluded. Audit consistency is not same-owner tamper resistance.

The technical reimport/persistence subset is verified. P3 overall and A05 remain partial because a real human-authored camera edit has not yet been preserved across a later agent edit. This run reimported unchanged GLBs; it does not establish a geometric A-to-B resource transition. G6 remains NOT_COMPLETE, art REVISE, and both native Linux regressions NOT_RUN. The consumed attempt must not be submitted again.
