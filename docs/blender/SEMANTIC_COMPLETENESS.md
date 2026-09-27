# Blender semantic completeness

Target runtime: **Blender 4.5.14 LTS**.

Semwright calls the first-party Blender driver semantically complete inside the managed persistent-authoring boundary defined here. Completeness does not mean mirroring every Python method, editor action or context-sensitive operator. It means an agent can discover, address, inspect and mutate the supported Blender authoring model without falling back to pixels, coordinates, arbitrary Python or generic `bpy.ops` invocation.

## Generic RNA substrate

The version-pinned substrate provides:

- allowlisted persistent `bpy.data` roots and generation-bound opaque refs;
- root-type and concrete RNA type discovery/description;
- paged datablock discovery and scalar query pushdown;
- typed property get/set/reset with bounds, enum and finite-number validation;
- pointer/collection traversal plus explicitly classified relation mutation;
- safe pointer set and collection link/unlink where Blender exposes those semantics;
- semantic rename and bounded JSON-compatible custom properties;
- bounded datablock lifecycle for roots with direct Blender APIs;
- stale-reference rejection after every semantic mutation.

Deep relation traversal means one persistent root can reach subobjects such as mesh vertices, modifiers, constraints, Action/F-Curve data, node sockets, pose bones, Mask points, Sequence strips, tracking markers and other nested RNA state.

## Authoring-domain overlays

| Domain | Status | Primary semantic surface |
| --- | --- | --- |
| Scenes / objects / collections | Managed | roots, typed Object+data creation, relations, rename, create/remove, curated common operations |
| View layers / timeline markers | Managed | add/remove/move ViewLayers and add/remove markers |
| Transforms and ordinary RNA state | Managed | `semantic.property.*` |
| Materials | Managed | material commands + RNA + node-tree traversal |
| Cameras / lights / light probes / worlds | Managed | datablock lifecycle + RNA properties/relations |
| Modifiers | Managed | modifier lifecycle + RNA properties |
| Constraints | Managed | Object and PoseBone constraint lifecycle + RNA properties |
| Blender 4.5 Actions | Managed | Action slots, layers, keyframe strips, channelbags and F-Curves |
| F-Curves | Managed | keyframe lifecycle, typed F-Curve modifiers and generic RNA editing |
| NLA | Managed | track/strip lifecycle + RNA |
| Sequence Editor | Managed | editor ensure; scoped media/datablock/meta/effect strips; strip modifiers; removal |
| Node graphs | Managed | type discovery, node/link lifecycle, NodeTree interface sockets/panels/reparenting |
| Mesh topology | Managed | bounded topology replace + vertex/edge/polygon refs |
| Mesh attributes / color attributes | Managed | attribute lifecycle + attribute-data refs |
| UV maps | Managed | UV layer lifecycle + UV data refs |
| Shape keys | Managed | shape-key lifecycle + KeyBlock refs |
| Vertex groups / weights | Managed | group lifecycle and bounded membership/weight mutation |
| Curves / splines | Managed | spline lifecycle + point refs/properties |
| Masks | Managed | Mask datablock, layer/spline/point lifecycle + point RNA properties |
| Armatures / bones | Managed | bounded Edit Mode bone lifecycle/parenting, Bone Collections, Pose constraints + RNA |
| Grease Pencil | Managed | legacy/modern persistent roots; layer/frame/drawing/stroke lifecycle + RNA |
| Hair Curves | Managed | curve add/remove/resize/reorder/type mutation + RNA |
| MetaBall | Managed | element lifecycle + RNA |
| Movie Tracking | Managed | tracking-object/track/marker authoring; marker removal; contained-track cleanup via object lifecycle |
| PointCloud and other persistent geometry IDs | Managed state | persistent root + RNA/property/attribute semantics; no generic operator fallback |
| Geometry Nodes | Managed | Nodes modifier + node-group datablock + node/interface semantics |
| Compositor / shader / texture node trees | Managed | node-tree traversal and generic node graph semantics |
| Scoped file-backed assets | Managed | allowlisted image/sound/font/movie/cache/volume loading from workspace |
| Render settings / still render | Managed | curated render commands + nested RNA settings |
| Add-ons / operators | Discovery only | bounded introspection; no generic invocation |

## Property classification

Every property in the managed RNA graph is classified as one of:

- `managed`: bounded semantic get/set/reset;
- `read_only`: inspectable but not mutable through the generic codec;
- `relation`: identity-bearing pointer/collection with an explicit mutation classification;
- `runtime_owned`: metadata/back-references/evaluated runtime state;
- `unsupported_by_design`: crosses code, filesystem, network or ambient-authority boundaries.

The CI-generated `RNA_COVERAGE.json` is produced by the pinned Blender 4.5.14 runtime. Certification requires:

- zero unclassified persistent ID types;
- zero unresolved relation target types;
- zero unavailable declared roots;
- at least 1,000 reachable authoring RNA types;
- no `*_OT_*` operator classes in the reachable authoring graph;
- no Window/Screen/Area/Region/Space/Operator/UI runtime types in that graph;
- distinct managed roots for `GreasePencil` and `GreasePencilv3`, with modern Layer/Frame/Drawing RNA reachable.

The full matrix is retained as a CI artifact instead of committing tens of thousands of generated property rows to Git.

## Deliberate authority boundary

Semantic completeness does **not** authorize:

- arbitrary Python, `eval`, `exec`, arbitrary imports or shell execution;
- generic `bpy.ops` invocation;
- animation-driver expressions or script-backed relations;
- Blender Text datablocks as executable source;
- arbitrary external-library linking/appending;
- arbitrary filesystem paths or URLs;
- unrestricted add-on execution;
- Window/Screen/Workspace/editor runtime state;
- network access.

Where Blender 4.5 exposes lifecycle only through context-sensitive operators and no dedicated context-safe wrapper exists, that lifecycle remains outside the generic semantic substrate rather than becoming an escape hatch. The bounded armature Edit Mode transaction is an explicit reviewed exception: the operator and transition are fixed internally and are not agent-selectable.
