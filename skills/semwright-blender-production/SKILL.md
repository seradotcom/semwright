---
name: semwright-blender-production
description: Author, inspect, measure and persist managed Blender assets through Semwright's typed native Composition profile, with explicit scope and evidence limitations.
---

# Blender production

This package is a development draft, not a production-readiness declaration. Discover the installed live catalog and strict argument schemas before use. Missing commands are a stop condition, not permission to substitute a script.

## Authoring loop

Inspect native source state first. Express bounded native entities, geometry, materials, bones, relationships and animation as `BlenderAuthoringSpec` data. Authoring aliases are not Project Graph logical IDs. Do not adopt an existing object by display name.

Call `driver.blender.composition.plan` with a typed intent, review its operations, dependencies and non-atomic write scope, and call `driver.blender.composition.apply` using the returned plan reference in the same authenticated session. A plan is not a policy grant. Do not supply an owner or principal, edit the plan, replay it, or silently replace a failed attempt with a new one.

Use inspect for bounded source-only readback. `composition.measure` explicitly distinguishes source and evaluated state: evaluation has a cost and requires its own mutation-capable authorization. Single-frame evaluation does not establish all-time correctness. Bounds intersection is not mesh collision evidence. Request human/visual review for aesthetics; no automatic beauty verdict exists.

Incremental `transform` intents require the managed island/entity and exact observed fingerprint. On drift, ambiguity, shared-resource surprises or linked/read-only data, stop and reconcile. This draft does not offer automatic repair, copy-on-write, remeshing or topology reduction. Do not invent a repair capability.

## Persistence and export

`composition.persist` creates a new `.blend` artifact in the owner-granted workspace; it is not overwrite or rollback. Native library serialization may expand indirect dependencies. Inspect the closure and preserve returned digest and inventory. Reopen in a fresh disposable driver process and compare actual fields; saving or returning an acknowledgment is not persistence verification.

Reuse `driver.blender.export.glb`. Review effective membership, parents, armatures, actions, modifiers and material/texture dependencies before export. Instancers, external/linked dependencies and unsupported closure must fail rather than silently export the whole scene. Keep the existing fixed exporter and its no-overwrite path.

Blender never writes a Godot project. Transfer artifacts through the public artifact route and D's public Godot API, recording distinct export/handoff/import/verification activities. No `send_to_godot` shortcut or hidden `.tscn` write.

`composition.validate` and `composition.verify` preserve required UNKNOWN. The draft's effect-conformance rule remains UNKNOWN until F's trusted native adapter is actually integrated; a structural check is not C/F pipeline acceptance. Read [typed authoring](references/typed-authoring.md) for the implemented draft boundary.
