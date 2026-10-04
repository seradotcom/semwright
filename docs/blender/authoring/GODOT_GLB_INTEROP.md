# Blender → Godot GLB interoperability contract

The Blender authoring work initially inspected Godot source SHA `557ad0b555d21f77d678fd760fb471dccfdee82d` without merging or editing Godot-owned paths. That historical inspection predated the later public cross-app artifact-handoff/import lane.

## What the Godot implementation already proves in source

The Godot typed authoring model has `AssetKind::Glb` with a declared relative file and SHA-256. Its store reads the declared input from its configured input root, checks the exact digest, enforces a 16 MiB input budget, validates GLB v2 framing/chunks, bounds JSON structure, and rejects external/data URI dependencies. The Godot compiler can realize an `Instance` of that GLB as a `PackedScene`.

Those implementation facts informed the contract below. They are not, by themselves, cross-app acceptance evidence.

## Current public boundary and missing acceptance

Godot now exposes a Broker-facing cross-app artifact handoff/import route and a dedicated cross-app GLB lane. The Blender side therefore no longer treats the API boundary itself as missing. The recorded E11 checkpoint remains blocked because no exact-SHA cross-app candidate had yet completed that public route with native Godot import/readback/semantic verification against an authentic final Blender artifact.

The Blender producer therefore must **not**:
- call `authoring::store::Store` directly;
- write into the Godot project directory or `.godot/imported`;
- manufacture a `.tscn` or import metadata;
- treat `driver.godot.asset.reimport` on a manually copied file as the Godot semantic authoring route;
- claim cross-app acceptance from the internal Godot `validate_asset` helper.

## Required Godot receipt for cross-app acceptance

For closure, the integration needs one exact-SHA public, policy-checked Godot flow with:
1. artifact/input identity and exact Blender GLB digest;
2. Godot project/base identity and owner session from the authenticated execution context;
3. declared `AssetKind::Glb` and bounded destination under Godot-owned grants;
4. Godot native import/readback after the editor/importer actually consumes it;
5. semantic observations for at least node/mesh count, transforms, skin/animation presence and material slots that the Godot integration can truthfully observe;
6. support/fidelity/UNKNOWN for unsupported glTF semantics rather than byte-equality claims;
7. a Project Graph receipt representing export → handoff → import → verification as distinct activities/revisions;
8. Effect Conformance evidence bound to the Godot native observation channel.

The existing `driver.blender.export.glb` remains the producer. Godot remains the importer/consumer. Neither driver receives the other's filesystem authority.

Until that exact-SHA Godot native cross-app evidence exists, the recorded cross-app gate remains `BLOCKED_DEPENDENCY`, not PASS and not unsupported.
