# Research baseline — Blender authoring

Date: 2026-09-28/29. Runtime target is Blender **4.5.14 LTS**, pinned and hash-checked by Semwright CI. Documentation informs implementation; the pinned runtime probe is the actual availability test.

## Blender references used

- Blender 4.5 Python API root: https://docs.blender.org/api/4.5/
- Blender 4.5 glTF manual: https://docs.blender.org/manual/en/4.5/addons/import_export/scene_gltf2.html
- `BlendDataLibraries.load/write`: https://docs.blender.org/api/4.5/bpy.types.BlendDataLibraries.html
- `Depsgraph` / evaluated IDs: https://docs.blender.org/api/4.5/bpy.types.Depsgraph.html
- `Mesh.from_pydata`, `validate`, loop triangles: https://docs.blender.org/api/4.5/bpy.types.Mesh.html
- math geometry utilities: https://docs.blender.org/api/4.5/mathutils.geometry.html
- Blender 4.5 Boolean modifier semantics: https://docs.blender.org/manual/en/4.5/modeling/modifiers/generate/booleans.html
- Blender Curve datablock/spline properties: pinned runtime RNA plus https://docs.blender.org/api/4.5/
- Blender 4.5 Principled BSDF Alpha/Emission: https://docs.blender.org/manual/en/4.5/render/shader_nodes/shader/principled.html
- action/slot/layer/channelbag/F-Curve availability is additionally checked by the existing pinned Blender RNA coverage lane.

Design decisions: source and evaluated state are distinct because depsgraph evaluation applies animation, constraints and modifiers. Typed topology is validated before `Mesh.from_pydata`; if native `Mesh.validate` says repair was necessary, the Blender authoring layer rejects the geometry instead of silently changing it.

AABB overlap is only broad-phase evidence. E's pairwise narrow phase works on evaluated world-space loop triangles, using segment/triangle intersection plus the documented 2D triangle helper for coplanar triangles. Budget/degeneracy returns UNKNOWN, and this method is not reused as a self-intersection claim.

`BlendDataLibraries.write` can expand indirect dependencies; therefore persistence requires closure checks and a fresh-process readback rather than assuming the requested collection is the exact serialized set. The glTF exporter can expand semantics through objects/modifiers/materials/textures/actions, so selection alone is not accepted as membership proof.

## Semwright references

Composition C0 defines Owner/BaseState/PreparedPlan/ChangeSet/PlanVault/reports. Project Graph P0 defines persistent graph identities/receipts/admission. Effect Conformance E0 defines effect predicates/evidence/evaluation. PR #154 provides the fixed GLB exporter/sealed-runtime probes. Blender authoring adds domain compilation/adapters only; it does not fork those authorities.

Historical Godot SHA `557ad0b…` was the design baseline: its internal store validated a hash-pinned, self-contained GLB and could realize it as PackedScene before a public Broker route existed. Godot later published a public cross-app artifact-handoff/import route. This checkpoint still records the Godot roundtrip as a dependency blocker until that public route has exact-SHA native acceptance against an authentic Blender artifact; private Godot code is never used as substitute evidence.
