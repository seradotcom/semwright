# Blender semantic completeness

Target runtime: **Blender 4.5.14 LTS**.

Semwright calls the first-party Blender driver semantically complete only inside the managed,
persistent authoring boundary defined here. This does not mean mirroring every Python method,
operator or editor action. It means an agent can inspect, address and mutate the supported
persistent Blender data model without falling back to pixels, coordinates, arbitrary Python or
generic `bpy.ops` invocation.

## Generic RNA substrate

The version-pinned substrate exposes:

- allowlisted persistent `bpy.data` roots and revision-bound opaque refs;
- root and concrete RNA type discovery/description;
- paged datablock discovery plus scalar query pushdown;
- property get/set/reset with typed bounds and enum validation;
- pointer/collection traversal and safe pointer/link/unlink mutation;
- semantic rename and bounded JSON-compatible custom properties;
- stale-reference rejection after every semantic mutation.

Relations can be traversed deeply, so one root ref can reach objects such as mesh vertices,
node sockets, pose bones, modifier settings, render settings and nested scene data.
## Authoring-domain overlays

| Domain | Status | Primary semantic surface |
| --- | --- | --- |
| Scenes / objects / collections | Managed | RNA roots, relations, rename, create/remove, curated object/collection commands |
| Transforms and ordinary RNA state | Managed | `semantic.property.*` |
| Materials | Managed | material commands + RNA + node-tree traversal |
| Cameras / lights / worlds | Managed | datablock lifecycle + RNA properties/relations |
| Modifiers | Managed | `modifier.add/remove` + RNA properties |
| Constraints | Managed | `constraint.add/remove` + RNA properties |
| Animation keyframes | Managed | `animation.keyframe.insert/delete` + Action/FCurve RNA inspection |
| Node graphs | Managed | node type discovery, add/remove/link/unlink + socket/property refs |
| Mesh topology | Managed | `mesh.geometry.replace` + vertex/edge/polygon refs |
| Mesh attributes / color attributes | Managed | `mesh.attribute.add/remove` + attribute-data refs |
| UV maps | Managed | `mesh.uv_layer.add/remove` + UV data refs |
| Shape keys | Managed | `shape_key.add/remove` + KeyBlock refs |
| Vertex groups / weights | Managed | vertex-group lifecycle and bounded weight mutation |
| Curves / splines | Managed | spline lifecycle + point refs/properties |
| Armatures / bones | Managed | bounded Edit Mode transaction for bone lifecycle/parenting + bone/pose RNA |
| Geometry Nodes | Managed | Nodes modifier + node-group datablock + generic node graph semantics |
| Compositor / shader nodes | Managed | node-tree traversal and generic node graph semantics |
| Scoped file-backed assets | Managed | allowlisted image/sound/font/movie/cache/volume loading from workspace |
| Render settings / still render | Managed | curated render commands + nested RNA settings |
| Add-ons / operators | Discovery only | bounded introspection; no generic invocation |
## Property classification

Every property reached through the managed RNA boundary is classified as one of:

- `managed`: bounded semantic get/set/reset;
- `read_only`: inspectable but RNA declares it non-mutable;
- `relation`: traversed as an identity-bearing pointer/collection;
- `runtime_owned`: metadata/back-references that are not persistent authoring state;
- `unsupported_by_design`: crosses a code, filesystem, network or ambient-authority boundary.

The CI-generated `RNA_COVERAGE.json` records this classification for the pinned Blender build
and lists any persistent ID type that has not been assigned either a managed root or a deliberate
exclusion. Final certification requires zero unclassified persistent ID types in the declared
boundary.

## Deliberate exclusions

Semantic completeness does **not** authorize:

- arbitrary Python, `eval`, `exec`, shell execution or arbitrary imports;
- generic `bpy.ops` invocation;
- animation-driver expressions or script-backed relations;
- Blender Text datablocks as executable source;
- arbitrary external-library linking/appending;
- arbitrary filesystem paths or URLs;
- unrestricted add-on execution;
- editor Window/Screen/Workspace runtime state;
- network access.

These exclusions are authority boundaries, not missing fallback implementations. A future provider
may add a separately reviewed capability for one of them without widening the generic RNA substrate.
