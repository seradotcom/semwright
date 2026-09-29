# Research baseline — Blender authoring

Date: 2026-09-28/29. Runtime target is Blender **4.5.14 LTS**, pinned and hash-checked by Semwright CI. Documentation informs implementation; the pinned runtime probe is the actual availability test.

## Blender references used

- Blender 4.5 Python API root: https://docs.blender.org/api/4.5/
- Blender 4.5 glTF manual: https://docs.blender.org/manual/en/4.5/addons/import_export/scene_gltf2.html
- `BlendDataLibraries.load/write`: https://docs.blender.org/api/4.5/bpy.types.BlendDataLibraries.html
- `Depsgraph` / evaluated IDs: https://docs.blender.org/api/4.5/bpy.types.Depsgraph.html
- `Mesh.from_pydata`, `validate`, loop triangles: https://docs.blender.org/api/4.5/bpy.types.Mesh.html
- math geometry utilities: https://docs.blender.org/api/4.5/mathutils.geometry.html
- action/slot/layer/channelbag/F-Curve availability is additionally checked by the existing pinned Blender RNA coverage lane.

Design decisions: source and evaluated state are distinct because depsgraph evaluation applies animation, constraints and modifiers. Typed topology is validated before `Mesh.from_pydata`; if native `Mesh.validate` says repair was necessary, E rejects the geometry instead of silently changing it.

AABB overlap is only broad-phase evidence. E's pairwise narrow phase works on evaluated world-space loop triangles, using segment/triangle intersection plus the documented 2D triangle helper for coplanar triangles. Budget/degeneracy returns UNKNOWN, and this method is not reused as a self-intersection claim.

`BlendDataLibraries.write` can expand indirect dependencies; therefore persistence requires closure checks and a fresh-process readback rather than assuming the requested collection is the exact serialized set. The glTF exporter can expand semantics through objects/modifiers/materials/textures/actions, so selection alone is not accepted as membership proof.

## Semwright references

A C0 owns Owner/BaseState/PreparedPlan/ChangeSet/PlanVault/reports. C P0 owns persistent graph identities/receipts/admission. F E0 owns effect predicates/evidence/evaluation. PR #154 owns the fixed GLB exporter/sealed-runtime probes. E adds Blender-domain compilation/adapters only; it does not fork those authorities.

D SHA `557ad0b…` was inspected for GLB consumer design: its internal store validates a hash-pinned, self-contained GLB and can realize it as PackedScene, but that is not a public Broker route. E therefore records the roundtrip as a dependency blocker rather than calling private D code.
