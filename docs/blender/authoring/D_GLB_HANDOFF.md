# Blender → Godot GLB handoff contract

E inspected D's published source SHA `557ad0b555d21f77d678fd760fb471dccfdee82d` without merging or editing D-owned paths.

## What D already proves in source

D's typed authoring model has `AssetKind::Glb` with a declared relative file and SHA-256. Its store reads the declared input from its configured input root, checks the exact digest, enforces a 16 MiB input budget, validates GLB v2 framing/chunks, bounds JSON structure, and rejects external/data URI dependencies. D's compiler can realize an `Instance` of that GLB as a `PackedScene`.

Those are implementation facts at the inspected SHA. They are not yet E11 acceptance evidence.

## Missing public boundary

At `557ad0b…`, E did not find a Broker-facing Godot Composition/authoring capability that accepts an artifact token or a host-granted GLB input and returns native import/readback evidence. The current published D coordination state also says native parsing/persistence/export acceptance is not established.

E therefore must **not**:
- call `authoring::store::Store` directly;
- write into D's project directory or `.godot/imported`;
- manufacture a `.tscn` or import metadata;
- treat `driver.godot.asset.reimport` on a manually copied file as D's semantic authoring route;
- claim E11 from D's internal `validate_asset` helper.

## Required D receipt for E11

When D publishes the route, E needs one public, policy-checked flow with:
1. artifact/input identity and exact Blender GLB digest;
2. D project/base identity and owner session from the authenticated execution context;
3. declared `AssetKind::Glb` and bounded destination under D's own grants;
4. Godot native import/readback after the editor/importer actually consumes it;
5. semantic observations for at least node/mesh count, transforms, skin/animation presence and material slots that D can truthfully observe;
6. support/fidelity/UNKNOWN for unsupported glTF semantics rather than byte-equality claims;
7. a C receipt representing export → handoff → import → verification as distinct activities/revisions;
8. F evidence bound to D's own native observation channel.

E's existing `driver.blender.export.glb` remains the producer. D remains the importer/consumer. Neither driver receives the other's filesystem authority.

Until that D capability and exact-SHA native evidence exist, E11 remains `BLOCKED_DEPENDENCY`, not PASS and not unsupported.
