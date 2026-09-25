# Godot capability surface

The Godot driver exposes **184 curated `driver.godot.*` capabilities**. Every advertised
capability has an operation-specific input and output schema, descriptor digest, route and
risk/idempotency classification. The catalog is checked against the production EditorPlugin
dispatch and runner routes. The goal is semantic domain coverage, not a one-tool-per-method
mirror of Godot's raw object API.

## Diagnostics, project and scene

- `doctor`, `session.list`
- `project.inspect`, `project.files`, `project.main_scene`
- `scene.inspect`, `scene.create`, `scene.open`, `scene.save`, `scene.reload`
- `scene.instantiate`, `snapshot.diff`

## Nodes, groups and project input

- `node.inspect`, `node.create`, `node.patch`, `node.remove`
- `node.rename`, `node.reparent`, `group.set`
- `input.list`, `input.set`, `input.remove`

Input actions preserve keyboard, mouse-button, gamepad-button and analog-axis bindings,
including device identity and signed axis direction. Generic node/resource operations remain
the bounded substrate for engine types that do not
need a dedicated semantic workflow. Domain operations below provide higher-level meaning for
the major authoring systems instead of requiring agents to guess raw property names.

## Resources, scripts and signals

- `resource.inspect`, `resource.create`, `resource.patch`, `resource.duplicate`
- `script.inspect`, `script.write`, `script.attach`, `script.detach`, `script.validate`
- `signal.list`, `signal.connect`, `signal.disconnect`
- `assets.status`, `assets.rescan`

Managed script writes are project-root confined, hash/precondition aware and reject `@tool`.
They remain code-execution risk because project scripts execute when the owner runs the project.

## Animation

- `animation.inspect`, `animation.create`, `animation.remove`
- `animation.track.add`, `animation.track.remove`
- `animation.keyframe.set`, `animation.keyframe.remove`
- `animation_tree.configure`

Track types are explicit and bounded; arbitrary method invocation is not exposed.

## 2D tile authoring

- `tilemap.inspect`, `tilemap.cell.set`, `tilemap.cell.erase`, `tilemap.clear`
- `tileset.inspect`, `tileset.configure`
- `tileset.atlas.create`, `tileset.tile.create`

Tile cell inspection is bounded and reports truncation. TileSet mutation is resource-backed
and saved through the normal Godot resource pipeline.

## 3D grid authoring

- `gridmap.inspect`, `gridmap.configure`
- `gridmap.cell.set`, `gridmap.cell.erase`, `gridmap.clear`
- `meshlibrary.inspect`
- `meshlibrary.item.create`, `meshlibrary.item.configure`, `meshlibrary.item.remove`

GridMap cells use explicit integer 3D coordinates and Godot's 24 orthogonal orientations.
MeshLibrary item edits are resource-backed and expose mesh plus navigation associations. More
complex item transforms/collision-shape arrays are intentionally deferred to the richer Variant
codec rather than accepting untyped transform payloads.

## Paths and curves

- `path.inspect`, `path.configure`
- `path.point.add`, `path.point.configure`, `path.point.remove`, `path.clear`
- `path.follow.inspect`, `path.follow.configure`

Path2D/Curve2D and Path3D/Curve3D share bounded Bézier point semantics while preserving
dimension-specific fields such as Curve3D tilt/closure/up vectors and PathFollow rotation modes.

## Navigation

- `navigation.region.inspect`, `navigation.region.configure`, `navigation.region.bake`
- `navigation.agent.inspect`, `navigation.agent.configure`
- `navigation.link.configure`

The same typed operations cover NavigationRegion/Agent/Link in both 2D and 3D. Vector
shape is validated against the target dimension, while region resources remain explicit
NavigationPolygon (2D) or NavigationMesh (3D). The surface models layers, costs, path
tolerances, avoidance and link endpoints without exposing arbitrary NavigationServer calls.

## Physics

- `physics.layers`
- `physics.body.inspect`, `physics.body.configure`
- `physics.area.inspect`, `physics.area.configure`
- `physics.joint.configure`
- `collision.shape.configure`

Body, Area, Joint and CollisionShape operations preserve 2D/3D semantics. RigidBody,
CharacterBody and StaticBody variants use dimension-correct velocity/angular types, while
Area gravity vectors and collision resources are validated against the target dimension.
Directional gravity and point-gravity center are mutually exclusive because Godot stores them
through the same underlying gravity vector; `gravity_point` selects which semantic
interpretation is active.

## Audio

- `audio.player.inspect`, `audio.player.configure`
- `audio.bus.inspect`, `audio.bus.create`, `audio.bus.configure`, `audio.bus.remove`
- `audio.effect.add`, `audio.effect.remove`

Audio bus edits are persisted as the project's default AudioBusLayout instead of remaining
editor-process-only state.

## Particles and VFX

- `particles.inspect`, `particles.configure`, `particles.restart`
- `particles.material.configure`

The emitter surface covers GPU/CPU 2D/3D nodes and a curated ParticleProcessMaterial subset.

## Rendering

- `camera.inspect`, `camera.configure`
- `light.inspect`, `light.configure`
- `environment.inspect`, `environment.configure`
- `material.standard.inspect`, `material.standard.configure`
- `shader.write`, `shader.attach`

Camera2D/3D, Light2D/3D, Environment and StandardMaterial3D have dedicated semantics rather
than relying exclusively on generic property patches.

## UI and themes

- `ui.layout`, `ui.control.inspect`, `ui.control.configure`, `ui.text.configure`
- `theme.inspect`, `theme.configure`, `theme.apply`

Theme mutation is typed across colors, constants, font sizes, fonts, icons, styleboxes and
type variations.

## Skeleton and rigging

- `skeleton.inspect`
- `skeleton.bone.add`, `skeleton.bone.configure`
- `skeleton.attachment.configure`

Bone inspection and mutation are bounded to 512 bones and expose hierarchy plus pose state.

## Project settings and autoloads

- `project.window.inspect`, `project.window.configure`
- `project.rendering.inspect`, `project.rendering.configure`
- `project.physics.inspect`, `project.physics.configure`
- `project.layers.inspect`, `project.layers.set`
- `autoload.list`, `autoload.add`, `autoload.remove`

Only curated project settings are writable. Autoload creation is classified as code-execution
risk because the registered script executes when the project is run.

## Asset import and export configuration

- `asset.inspect`, `asset.dependencies`, `asset.reimport`
- `asset.import.inspect`, `asset.import.configure`
- `export.preset.list`, `export.preset.inspect`, `export.preset.configure`

Import configuration can only modify existing scalar keys in an existing `.import` sidecar
and then reimports through EditorFileSystem. Export configuration is limited to
`export_presets.cfg`; Semwright never reads or writes `.godot/export_credentials.cfg`.

## Localization

- `localization.inspect`, `localization.configure`
- `translation.inspect`, `translation.create`
- `translation.message.set`, `translation.message.remove`

Translation resources are normal Godot resources and project registration is persisted through
ProjectSettings.

## Deep AnimationTree authoring

- `animation_tree.inspect`
- `animation_tree.state.add`, `animation_tree.state.remove`
- `animation_tree.transition.add`, `animation_tree.transition.configure`,
  `animation_tree.transition.remove`
- `animation_tree.parameter.set`
- `animation_tree.node.inspect`, `animation_tree.node.configure`
- `animation_tree.blend_tree.inspect`
- `animation_tree.blend_tree.node.add`, `animation_tree.blend_tree.node.configure`,
  `animation_tree.blend_tree.node.remove`
- `animation_tree.blend_tree.connection.set`
- `animation_tree.blend_space.inspect`, `animation_tree.blend_space.configure`
- `animation_tree.blend_space.point.add`, `animation_tree.blend_space.point.configure`,
  `animation_tree.blend_space.point.remove`
- `animation_tree.blend_space.triangle.add`, `animation_tree.blend_space.triangle.remove`

State machines, BlendTrees and named 1D/2D blend spaces can be addressed recursively through a
bounded semantic graph path. Node creation is limited to explicit built-in AnimationNode kinds;
BlendTree connections and BlendSpace points/triangles have typed schemas. Transitions expose
condition, advance mode, priority, reset, switch mode and cross-fade semantics without arbitrary
method dispatch.

## Multiplayer authoring

- `multiplayer.spawner.inspect`, `multiplayer.spawner.configure`
- `multiplayer.spawner.scene.add`, `multiplayer.spawner.scene.remove`
- `multiplayer.synchronizer.inspect`, `multiplayer.synchronizer.configure`
- `multiplayer.replication.property.add`, `multiplayer.replication.property.configure`,
  `multiplayer.replication.property.remove`

These capabilities author scene replication metadata only. They do not create peers, open
network sockets or expose arbitrary RPC execution.

## Editor state

- `editor.state`
- `editor.selection.get`, `editor.selection.set`
- `editor.run.start`, `editor.run.stop`

Editor run-start is explicitly code-execution risk. Selection and state use EditorInterface and
EditorSelection rather than coordinate automation.

## API introspection and generic semantic substrate

- `api.search`, `api.describe`
- `project.class.list`, `project.class.describe`

ClassDB introspection is version-bound to the connected Godot engine and exposes bounded class,
property, method, signal, enum and default-value metadata. Script-defined `class_name` types
are discovered separately through ProjectSettings and Script metadata. Discovered methods are
**descriptive only**: the driver does not expose dynamic method invocation.

Generic `node.patch` and `resource.patch` writes are checked against the runtime property
metadata before mutation. Their Variant codec supports scalar values plus StringName, NodePath,
vectors and integer vectors, Rect2/Rect2i, Transform2D/Transform3D, Quaternion, Plane, AABB,
Basis, Projection, Color, bounded Array/Dictionary envelopes, packed arrays, project Resource
references and current-scene NodeRef values. Unsupported or non-addressable objects are
reported as opaque on reads rather than becoming executable handles.

Bounded values round-trip directly. Truncated collection outputs carry explicit truncation
metadata and are intentionally rejected by strict write schemas to prevent lossy mutation.

## Headless runner and artifacts

- `project.validate`
- `project.run_test`
- `export.pack`
- `export.build`
- `movie.capture`

Runner operations execute only the owner-configured digest-pinned Godot binary through
allowlisted argument plans, a scrubbed environment, private HOME, bounded output, timeouts,
process-group cleanup and Driver Protocol v2 cancellation/progress/artifact frames.

## Semantic-completeness boundary

This surface intentionally does **not** mirror every ClassDB method. Arbitrary `Object.call`,
OS execution and unrestricted GDScript evaluation remain unavailable. The curated domain layer
covers the major Godot authoring systems, while the generic substrate provides broad bounded
Variant transport and version-bound API discovery without turning introspection into execution.

Remaining completeness work is infrastructure-level: stronger provider-owned refs, companion
plugin distribution, loopback-only network authority, managed secrets/secondary executables,
per-operation persistent-driver budgets and cross-platform Driver Host certification.

## Acceptance evidence

The real acceptance harness uses the production Rust driver and Godot 4.7.2-stable to create,
mutate, persist, reload, validate and run a disposable Lab Room. It exercises representative
operations from every first-class semantic domain, exports a PCK, checks runtime output for
Godot errors, verifies cooperative cancellation and observes child events.
