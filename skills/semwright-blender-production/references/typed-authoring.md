# Typed authoring draft

The live JSON Schema is authoritative. A create request is `{ "intent": { "kind": "create", "spec": ... } }`. Fields in `spec` are version, collection, meters_per_unit, materials, entities, relations and animation. Unknown fields are rejected. Fixtures under the repository's `fixtures/blender-authoring/` are development inputs, not hidden authoring scripts or evidence of success.

Lengths use explicit meters_per_unit; rotations are radians. Supported draft geometry includes box, cylinder, bounded mesh arrays, empty, armature, camera, area light and explicit shared mesh instance. Bevel, mirror, subdivision and array remain non-destructive. Materials are opaque base-color/roughness/metallic PBR, not arbitrary shaders. Skinning uses explicit bounded weights and declared bones. Animation is bounded linear keyframes in the baseline's slotted Actions, using A's media-time Rate.

The runtime is Blender 4.5.14 LTS inside the existing Linux Driver Host sandbox. New Composition calls require authenticated protocol-v2 context and negotiated progress/cancellation support. Legacy protocol-v1 capabilities remain separate; v1 cannot manufacture an authoring Owner. External applications and authoring files are not automatically safe merely because they are native.

Geometry arrays are data; Python source, expression strings, generic RNA method names, callbacks, plugins and downloads are not valid authoring inputs. A fixed compiler implementation in the driver is different from accepting caller code.

Requirements not completed include general material node networks/texture authoring, copy-on-write, NLA, narrow-phase mesh intersection, repair, full pagination, host-admitted C receipts across export/handoff/import, Godot roundtrip, full hostile/fuzz and packaging acceptance. F effect evaluation is now wired to trusted native readback, but that does not close those other obligations.
