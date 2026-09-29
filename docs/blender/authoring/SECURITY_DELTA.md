# Security delta

## Authority retained

All public writes remain Broker → policy → Driver Host → Blender. `Owner.session` is constructed from `DriverExecutionContext`; no authoring argument contains owner, principal, filesystem root or executable. A Composition plan is data held in A's `PlanVault`; apply revalidates the native source fingerprint immediately before side effects and consumes the plan before mutation. Replay is denied.

No arbitrary Python, generic operator name, shell, shader source, driver expression, callback, addon install, URL download or Godot project write is introduced. Python exists only as fixed first-party backend implementation staged by the sealed Rust driver, analogous to the existing bridge.

## Failure semantics

The native batch is explicitly `NonAtomicSequence`. If apply/cancellation fails after reservation, the driver kills its Blender child, marks its session poisoned and records an UNKNOWN attempt; it does not retry, refund budget or claim rollback. A successful native batch is separately observed before F can evaluate it.

Manual edits change the source projection; managed collection marker/fingerprint mismatch is drift. Linked or override content is denied for mutation. Shared mesh mutation through instance material/modifier changes is rejected until explicit copy-on-write exists.

## Export and persistence

The existing fixed GLB exporter remains the only export implementation. E inventories bounded object/data/material/action/image dependencies before selection. Unsupported dependencies fail closed. Export still uses Blender's native operator internally; caller cannot choose another operator or exporter arguments.

`composition.persist` creates a new `.blend` using `libraries.write`; the official API documents that indirect references are expanded, so E inventories a closed managed island and does not claim that the requested collection alone proves serialized scope. It never overwrites an existing destination. Fresh-process reopen uses the owner-granted workspace and the driver's `--disable-autoexec` Blender process.

## Known open security obligations

No proof of perfect sandboxing or TOCTOU elimination is claimed. Descriptor-relative file confinement is not added here. Texture/linked-library adversarial fixtures, full paginated observations, narrow-phase self-intersection, hostile active-handler/import cases, fuzz, packaging and Godot reimport remain open acceptance work. F head `42204ac…` currently has a CI lint failure in its own test, so E records its API as consumed but not independently certified.
