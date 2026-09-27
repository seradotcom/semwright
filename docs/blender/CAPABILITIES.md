# Blender semantic capabilities

The catalog deliberately avoids one tool per Blender property. Broad coverage comes from typed RNA refs plus focused lifecycle overlays. The combined semantic branch exposes **139 Blender capabilities total**: 122 explicitly wired semantic/introspection/authoring descriptors plus 17 pre-existing curated operations, with no name overlap. Most Blender breadth is still behind the generic semantic substrate rather than one tool per RNA property.

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
- `driver.blender.semantic.object.create`
- `driver.blender.semantic.datablock.create/remove`
- `driver.blender.asset.load`

Refs are generation-bound; semantic mutation rotates the generation and stale refs fail closed.

## High-level authoring overlays

- Object/data creation and collection relationships.
- Modifiers and Object/PoseBone constraints.
- Blender 4.5 Action slots/layers/keyframe strips/channelbags/F-Curves.
- F-Curve keyframes and built-in F-Curve modifiers.
- NLA track/strip lifecycle.
- Scene ViewLayers and timeline markers.
- Sequence Editor media/datablock/meta/effect strips and strip modifiers.
- Node graphs, links and NodeTree interface sockets/panels/reparenting.
- Mesh topology, attributes, UV layers, vertex groups and weights.
- Shape keys.
- Curve splines.
- Masks: layers, splines and points.
- Armature bones, Bone Collections and Pose constraints.
- Grease Pencil layers/frames/drawings/strokes.
- Hair Curves add/remove/resize/reorder/type mutation.
- MetaBall elements.
- Movie Tracking objects/tracks/markers.
- Scoped workspace asset loading.
- Render settings, still render and scoped .blend open/save.

The original curated scene/object/material/render/file capabilities remain for ergonomic common cases. RNA/operator/add-on introspection remains bounded and read-only; it does not grant generic operator execution.
