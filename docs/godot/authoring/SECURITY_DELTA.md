# Godot semantic authoring security delta

## New active surface

D adds semantic project authoring and native verification. `composition.native.verify`, `composition.native.query`, `composition.native.tracks.page` and `composition.native.keys.page` are CodeExecution because all launch pinned Godot on provider-generated code; query/paging are read-only with respect to canonical project state but are not relabeled as ordinary low-risk reads. They are advertised only with authoring grants plus a runner and require authenticated Driver Host context plus an A-issued plan. Native query accepts only one managed logical node key or persistent `res://` resource path and at most 32 lowercase allowlisted property names; filtering occurs inside the provider after trusted inspect, so the model does not receive the complete projection.

## Enforced boundaries

Intent decoding rejects unknown fields, oversized collections, invalid IDs, non-finite values, arbitrary scripts/plugins/executable paths, reflective method/callback payloads and caller GDScript. Behavior is a closed typed IR and generated GDScript comes from fixed templates. Event/tick/entity/spawn limits bound generated behavior but are not represented as a sandbox for arbitrary external Godot code. Supplied assets are hash-pinned and structurally validated before planning writes; GLB accepts only bounded version-2 self-contained JSON/BIN chunks and rejects any external or data URI even when the caller supplies the matching digest.

Output, state, input and artifact roots are owner configuration. Managed source I/O rejects traversal, symlinks, hardlinks/special files and implicit adoption of non-empty directories. Store hashes turn external edits into DIVERGED; partial publication requires explicit reconciliation. Native run/export requires IN_SYNC and executes from a verified disposable copy so import caches do not alter canonical managed sources.

The fixed observer accepts only a bounded managed scene, declared input actions, ticks/checkpoints/variable names and capture flag. It exposes no OS.execute, expression execution, arbitrary callback or shell surface. Observation JSON is strict-decoded and request/source/process/runtime bound. Trusted native result construction and F admission entry points are crate-private.

Save/reopen requires distinct native process IDs and nonces. F Reopened compares stable projections and bytes and requires unchanged external dependency sentinels. Animation paging cursors bind the managed source fingerprint plus normalized native projection, excluding process-local nonce/PID; keyframe cursors additionally bind the exact player/library/animation/track selector. Any source, projected-content or selected-track change invalidates continuation. Export presets exclude authoring metadata, addons and source maps; acceptance also scans and launches the exported executable in a cleared environment.

## Remaining risk and non-claims

Pinned Godot still executes generated GDScript and imported resources; sandboxing reduces but does not eliminate upstream-engine risk. Visual/aesthetic correctness, arbitrary-program reachability, bit-identical physics, malicious third-party projects and complete shader semantics are not proven. Fixture success is not independent security certification. Hostile/property and exact-SHA native gates remain required, and D does not close R16.
