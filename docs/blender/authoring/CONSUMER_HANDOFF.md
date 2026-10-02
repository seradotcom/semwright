# Consumer handoff — E on A/C/F/D

E consumes A C0 `26602e4b…`, C P0 `6ee52b4…`, F source through `5ed7d0f…`, and the GLB exporter from PR #154 head `74671c1…`. Consumption is pinned source, not automatic certification of another role's release state.

## F effect evidence

Every E `PreparedPlan` pins `dependencies["effects.contract"]` to the real F `EffectContract::digest`; A's required rules are exactly F's required rules. E's compiled `EvidenceAdapter` derives owner/provider-session/generation from the authenticated execution and native post-state, then calls F `collect` and `evaluate`. Client JSON cannot construct a trusted `EvidenceBatch`.

Create verifies both typed native readback and preservation of the whole-scene `blender-source-projection-v4` outside the newly managed island. v4 includes managed mesh attributes, source shading state, polygon-normal evidence and axis-selective COPY_LOCATION constraint state; consumers must not compare it as though it were v3. Transform/repair verifies the requested transform against independent native readback. Missing/changed evidence yields FAIL/UNKNOWN under A/F semantics rather than being normalized to PASS.

## C Project Graph

E uses C's real `ProjectId`, `LogicalAssetId`, `AssetRevision`, `RevisionPin`, `OperationIdentity`, `ExecutionReceipt` and `ReceiptAdapter`. Local spec IDs and Blender markers never become Project Graph IDs.

The native E2E obtains the actual apply descriptor digest from the live provider catalog, the actual driver binary digest, and retains the exact Broker request ID. Host-owned logical IDs plus the F-verified report form an `ExecutionReceipt` candidate; only C's registered adapter admits it. Coverage remains incomplete for cross-app provenance until export/handoff/D-import/D-verification are represented as separate activities.

## D Godot

E never writes `.tscn` or calls D's private store/compiler. The existing Blender GLB capability produces the artifact. The original D inspection at `557ad0b…` predated D's public cross-app artifact-handoff/import lane; D now owns that Broker-facing route. E11 remains `BLOCKED_DEPENDENCY` for a different reason: no exact-SHA D run has yet completed native Godot import/readback/semantic verification against an authentic final E artifact. The evidence contract remains documented in `D_GLB_HANDOFF.md`.

## Consumers of E

Use the live catalog and Skill requirements, not names copied from this Markdown. Treat `composition.inspect.page` cursors as single-use provider state. Use `mesh_copy` only when isolation is explicitly intended; use `mesh_instance` when native sharing is intended. Never infer collision from AABB overlap or artistic quality from a structural verification report.
