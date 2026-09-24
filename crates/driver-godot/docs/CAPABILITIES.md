# Godot capability surface

The Godot driver exposes 53 curated `driver.godot.*` capabilities. Every advertised
capability has an operation-specific input and output schema, descriptor digest, route and
risk/idempotency classification. The catalog is checked against the production EditorPlugin
dispatch and runner routes.

## Diagnostics, project and scene

- `doctor`, `session.list`
- `project.inspect`, `project.files`, `project.main_scene`
- `scene.inspect`, `scene.create`, `scene.open`, `scene.save`, `scene.reload`
- `scene.instantiate`, `snapshot.diff`

## Nodes, groups and project input

- `node.inspect`, `node.create`, `node.patch`, `node.remove`
- `node.rename`, `node.reparent`, `group.set`
- `input.list`, `input.set`, `input.remove`

Input actions are read and persisted through `ProjectSettings input/*`. EditorPlugin
`InputMap` state is deliberately not treated as the project InputMap.
## Resources, scripts and signals

- `resource.inspect`, `resource.create`, `resource.patch`, `resource.duplicate`
- `script.inspect`, `script.write`, `script.attach`, `script.detach`
- `script.validate`
- `signal.list`, `signal.connect`, `signal.disconnect`
- `assets.status`, `assets.rescan`

Managed script writes are project-root confined, hash/precondition aware and reject
`@tool`. They are still code-execution risk because project scripts execute when the owner
runs the project.

## Animation and visual state

- `animation.inspect`, `animation.create`, `animation.remove`
- `animation.track.add`, `animation.track.remove`
- `animation.keyframe.set`, `animation.keyframe.remove`
- `animation_tree.configure`
- `physics.layers`, `ui.layout`
- `shader.write`, `shader.attach`

The animation surface intentionally covers bounded track/keyframe operations rather than
arbitrary method invocation on Godot objects.
## Headless runner and artifacts

- `project.validate`
- `project.run_test`
- `export.pack`
- `export.build`
- `movie.capture`

Runner operations execute only the owner-configured digest-pinned Godot binary through
allowlisted argument plans, a scrubbed environment, private HOME, bounded output, timeouts,
process-group cleanup and Driver Protocol v2 cancellation/progress/artifact frames.

`export.build` requires installed export templates. `movie.capture` requires an explicitly
configured display; the runner never inherits DISPLAY implicitly.

## Acceptance evidence

The real acceptance harness uses the production Rust driver and Godot 4.7.2-stable to create,
persist, reload, validate and run a disposable 3D Lab Room. It also exports a PCK, checks
runtime output for Godot errors, verifies cooperative cancellation and observes child events.
Hosted CI repeats this against the pinned official Linux x86_64 Godot binary.
