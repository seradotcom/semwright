# Godot semantic authoring integration

## Authority and dependencies

D extends the existing first-party Godot driver. It does not define a second Composition kernel, policy engine, artifact transport or project graph. Plans, owners, base states, budgets and verification reports are A contracts. Durable project/file identities and apply receipts use C P0. Effect predicates, collection and evaluation use F. PR #176 is D's development PR; merge to main and R16 closure are outside D authority.

driver.godot.composition.plan is the semantic entry point. It advertises the existing generic artifact input classes, including artifact-in:model/3d. Bytes from Blender or another producer must first cross the Broker generic artifact.handoff into the configured Godot input grant with a pinned digest. D never reads another driver's private output path.

## Product route

The managed loop is inspect -> plan -> apply -> measure -> validate -> optional bounded repair -> verify. A plan is owner/session bound in PlanVault and is not permission by digest. Apply writes only provider-managed files under configured output/state grants. Human drift is DIVERGED and blocks overwrite, native execution and export.

`driver.godot.composition.native.verify` is a combined authoring+runner CodeExecution route. The caller supplies an issued plan_id, managed scene ID and typed inspect/persistence/play scenario. The driver resolves Owner, ProjectId, plan/intent digests and current source fingerprint server-side, requires IN_SYNC, copies verified managed sources to a private directory, runs pinned Godot import and the fixed observer, then evaluates through F. Native result admission is crate-private and trusted results are not deserializable from client JSON. `driver.godot.composition.native.tracks.page` and `driver.godot.composition.native.keys.page` reuse that authenticated inspect path but return only bounded collection pages plus F evaluation. Track cursors bind source fingerprint and normalized native projection; key cursors additionally bind the exact player/library/animation/track selector, so a new process can continue only while the observed content and requested collection are unchanged.

Persistence uses two Godot processes: save candidate then reopen. External dependency sentinels must remain byte-identical. Play accepts only declared InputMap actions, bounded ticks/checkpoints/variables and optional frame digest capture. Native readback preferences remain UNKNOWN when enumeration is incomplete; required source rules remain authoritative for source verification.

Existing project.validate, project.run_test and export routes accept either a paired configured project ID or one logical managed_project, never a path. Managed execution reopens Store, requires IN_SYNC and stages verified bytes. Export excludes authoring manifest/spec/source maps/addons and the acceptance test launches the package with a clean HOME and no editor/Semwright process.

## Cross-role handoffs and limits

C consumes D's authenticated apply receipt and retains UNKNOWN frontiers. F native evaluation is returned with native readback but does not become a grant. E may hand off a successful file-backed GLB through artifact.handoff; D validates GLB v2/digest, realizes it as PackedScene, imports it and can inspect meshes/materials/skeletons/animations/dependencies. The E-produced GLB cross-app run remains open until E publishes a successful artifact.

The branch does not claim arbitrary adoption of non-empty human projects, cross-app transactions, exactly-once native execution, complete rollback, sandbox perfection, Windows/macOS native authoring or completed Blender-to-Godot acceptance. Missing native execution, incomplete enumeration or drift remains UNKNOWN/FAIL.
