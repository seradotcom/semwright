# Godot capability surface

The Godot driver exposes **103 curated `driver.godot.*` capabilities**. Every advertised
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

Generic node/resource operations remain the bounded substrate for engine types that do not
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

## Navigation

- `navigation.region.inspect`, `navigation.region.configure`, `navigation.region.bake`
- `navigation.agent.inspect`, `navigation.agent.configure`
- `navigation.link.configure`

The surface models layers, costs, path tolerances, target position, avoidance and link
endpoints without exposing arbitrary NavigationServer calls.

## Physics

- `physics.layers`
- `physics.body.inspect`, `physics.body.configure`
- `physics.area.inspect`, `physics.area.configure`
- `physics.joint.configure`
- `collision.shape.configure`

Body configuration distinguishes RigidBody3D, CharacterBody3D and StaticBody3D semantics.

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
OS execution and unrestricted GDScript evaluation remain unavailable. The next completeness
layers are API introspection, richer Variant encoding, project/import/export configuration,
autoloads, deeper AnimationTree graphs and selected multiplayer/editor-state semantics.

## Acceptance evidence

The real acceptance harness uses the production Rust driver and Godot 4.7.2-stable to create,
mutate, persist, reload, validate and run a disposable Lab Room. It exercises representative
operations from every first-class semantic domain, exports a PCK, checks runtime output for
Godot errors, verifies cooperative cancellation and observes child events.
