# Blender semantic capabilities

The capability catalog keeps a small discovery surface and puts breadth behind typed RNA refs
instead of generating one tool per Blender property.

## Generic semantic substrate

- `driver.blender.semantic.summary`
- `driver.blender.semantic.types`
- `driver.blender.semantic.type.describe`
- `driver.blender.semantic.rna.describe`
- `driver.blender.semantic.objects`
- `driver.blender.semantic.query`
- `driver.blender.semantic.object.describe`
- `driver.blender.semantic.relations`
- `driver.blender.semantic.property.get/set/reset`
- `driver.blender.semantic.relation.set/link/unlink`
- `driver.blender.semantic.rename`
- `driver.blender.semantic.custom.list/get/set/remove`
- `driver.blender.semantic.datablock.create/remove`
- `driver.blender.asset.load`
## Authoring overlays

- modifiers and constraints: add/remove;
- animation: keyframe insert/delete;
- node graphs: type discovery, node add/remove, link/unlink;
- mesh: summary, bounded topology replace, attributes and UV layers;
- vertex groups: lifecycle and bounded weight set/remove;
- shape keys: add/remove;
- curves: spline add/remove;
- armatures: bone add/remove/parent set.

The original curated scene/object/material/render/file capabilities remain for ergonomic common
operations. Bounded RNA/operator/add-on introspection remains read-only and does not grant generic
operator execution.
