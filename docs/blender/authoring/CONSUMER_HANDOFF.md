# Blender authoring — consumed subsystem contracts

Blender authoring consumes Composition C0 `26602e4b…`, Project Graph P0 `6ee52b4…`, Effect Conformance source through `5ed7d0f…`, and the GLB exporter from PR #154 head `74671c1…`. Consumption is pinned source, not automatic certification of another subsystem's release state.

## Effect evidence

Every Blender `PreparedPlan` pins `dependencies["effects.contract"]` to the real F `EffectContract::digest`; Composition required rules are exactly the Effect Conformance required rules. The compiled `EvidenceAdapter` derives owner/provider-session/generation from the authenticated execution and native post-state, then calls Effect Conformance `collect` and `evaluate`. Client JSON cannot construct a trusted `EvidenceBatch`.

Create verifies both typed native readback and preservation of the whole-scene `blender-source-projection-v4` outside the newly managed island. v4 includes managed mesh attributes, source shading state, polygon-normal evidence and axis-selective COPY_LOCATION constraint state; consumers must not compare it as though it were v3. Transform/repair verifies the requested transform against independent native readback. Missing/changed evidence yields FAIL/UNKNOWN under shared Composition/Effect Conformance semantics rather than being normalized to PASS.

## Project Graph

Blender authoring uses the Project Graph `ProjectId`, `LogicalAssetId`, `AssetRevision`, `RevisionPin`, `OperationIdentity`, `ExecutionReceipt` and `ReceiptAdapter`. Local spec IDs and Blender markers never become Project Graph IDs.

The native E2E obtains the actual apply descriptor digest from the live provider catalog, the actual driver binary digest, and retains the exact Broker request ID. Host-owned logical IDs plus the F-verified report form an `ExecutionReceipt` candidate; only the registered Project Graph adapter admits it. Coverage remains incomplete for cross-app provenance until export/handoff/Godot-import/Godot-verification are represented as separate activities.

## Godot

Blender never writes `.tscn` or calls Godot private store/compiler internals. The existing Blender GLB capability produces the artifact. The original Godot inspection at `557ad0b…` predated D's public cross-app artifact-handoff/import lane; Godot now owns that Broker-facing route. The recorded E11 requirement remains `BLOCKED_DEPENDENCY` for a different reason: no exact-SHA Godot run has yet completed native Godot import/readback/semantic verification against an authentic final Blender artifact. The evidence contract remains documented in `D_GLB_HANDOFF.md`.

## Blender consumers

Use the live catalog and Skill requirements, not names copied from this Markdown. Treat `composition.inspect.page` cursors as single-use provider state. Use `mesh_copy` only when isolation is explicitly intended; use `mesh_instance` when native sharing is intended. Never infer collision from AABB overlap or artistic quality from a structural verification report.
