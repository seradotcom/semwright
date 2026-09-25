# Godot semantic-domain completeness

This matrix defines the boundary for the curated authoring layer. "Covered" means the domain has typed inspection and/or mutation semantics beyond relying only on raw property names. It does not mean Semwright mirrors every ClassDB method.

| Domain | Status | Primary semantic surface |
| --- | --- | --- |
| Projects/scenes/nodes/resources | Covered | project.*, scene.*, node.*, resource.* |
| 2D tile authoring | Covered | tilemap.*, tileset.* |
| Navigation 2D/3D | Covered | navigation.region.*, navigation.agent.*, navigation.link.* |
| Physics 2D/3D | Covered | physics.*, collision.shape.* |
| Input authoring | Covered | input.* including keyboard, mouse and gamepad bindings |
| AnimationPlayer | Covered | animation.*, tracks and keyframes |
| AnimationTree/state machines | Covered | animation_tree.* state, transition and parameter semantics |
| Rendering | Covered | camera.*, light.*, environment.*, material.standard.*, shader.* |
| Audio | Covered | audio.player.*, audio.bus.*, audio.effect.* |
| Particles/VFX | Covered | particles.* |
| UI/themes | Covered | ui.*, theme.* |
| Skeleton/rigging | Covered | skeleton.* plus generic node/resource substrate |
| Asset/import pipeline | Covered | asset.*, asset.import.* |
| Export authoring | Covered for existing presets | export.preset.* plus bounded runner export |
| Localization | Covered | localization.*, translation.* with translation contexts |
| Multiplayer scene metadata | Covered | multiplayer.spawner.*, synchronizer.*, replication.* |
| Editor semantic state | Covered | editor.state, selection.*, run.* |
| Managed GDScript | Covered within trust boundary | script.*; @tool is rejected |
| Signals/groups | Covered | signal.*, group.set |
| Autoloads/project settings | Covered | autoload.*, curated project.* settings |

## Deliberate exclusions from this layer

XR runtime/device control, arbitrary editor plugins, arbitrary Object.call, arbitrary GDScript evaluation, C#/.NET execution, GDExtension/native-code loading and unrestricted NavigationServer/RenderingServer/PhysicsServer calls are not part of semantic-domain completeness. They cross platform, executable-code or ambient-authority boundaries and require separate provider/security designs.

Niche engine classes that do not justify a dedicated workflow remain reachable through bounded node/resource primitives where their values fit the supported semantic value model. Broader Variant support, ClassDB/API search and describe, and stronger provider-owned refs belong to the generic semantic substrate pass rather than this domain layer.
