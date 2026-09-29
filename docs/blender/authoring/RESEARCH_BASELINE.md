# Research baseline — Blender authoring

Date: 2026-09-28/29. Runtime target is Blender **4.5.14 LTS** as verified by the existing Semwright workflow, not an arbitrary latest build.

## Official Blender sources used

- Blender 4.5 Python API root: https://docs.blender.org/api/4.5/
- Blender 4.5 glTF manual: https://docs.blender.org/manual/en/4.5/addons/import_export/scene_gltf2.html
- Blender Python `BlendDataLibraries.load/write`: https://docs.blender.org/api/4.5/bpy.types.BlendDataLibraries.html
- Blender 4.5 `Depsgraph` / evaluated ID API: https://docs.blender.org/api/4.5/bpy.types.Depsgraph.html
- Blender 4.5 `Mesh` API (`from_pydata`, `validate`): https://docs.blender.org/api/4.5/bpy.types.Mesh.html
- Blender 4.5 action/slot/layer APIs are additionally runtime-probed by Semwright's existing pinned RNA coverage before native acceptance.

Decisions: evaluated state is kept separate from source state because evaluated depsgraph applies animation/constraints/modifiers. `Mesh.from_pydata` input is validated by our typed model and then native `Mesh.validate`; if Blender repairs invalid geometry, E rejects instead of silently accepting the repair. `BlendDataLibraries.write` can expand indirectly referenced datablocks, therefore persistence requires dependency inventory and fresh-process readback rather than assuming the requested collection is the full write scope. The glTF exporter supports meshes/materials/textures/animation with mode-specific behavior, so selection alone is not treated as complete membership proof.

## Semwright sources

A C0 provides `Owner`, `BaseStateSet`, `PreparedPlan`, `ChangeSet`, `PlanVault` and reports. C P0 provides durable project identities/receipts. F E0 provides effect contracts, trusted evidence collection/evaluation and enumeration transcript rules. PR #154 provides the fixed GLB exporter and sealed-runtime probes. E adds no second copies of those authorities.

The authoritative availability proof remains the exact pinned runtime CI. Documentation informs design; it does not certify that a symbol works in Semwright.
