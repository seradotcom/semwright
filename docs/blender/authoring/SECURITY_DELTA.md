# Security delta

## Authority retained

All public writes remain Broker → policy → Driver Host → Blender. The Registry-facing `composition.plan` input schema is a bounded transport envelope rather than a duplicated deep authoring schema; after transport validation, the driver performs strict serde decoding into `AuthoringIntent`, validates the full `BlenderAuthoringSpec`, and pins the full schema digest in `ProfileIdentity` before any plan can enter `PlanVault`. `Owner.session` comes from `DriverExecutionContext`; no authoring argument contains owner, principal, executable or filesystem root. Plans are A `PreparedPlan` data stored in A's `PlanVault`, and apply revalidates native state before mutation. Replay is denied.

No caller-selected Python, generic operator, shell command, shader source, driver expression, callback, addon install, URL download or Godot project write is introduced. The sealed Blender child is always launched with `--background --factory-startup --disable-autoexec --python <fixed bridge>`; a driver-binary unit test asserts those security arguments and rejects enable-autoexec/interactive Python escape flags. Texture inputs are clean relative workspace paths with codec/size bounds and lowercase SHA-256 pins; the fixed backend hashes before load and again during closure/readback so a changed external asset becomes stale instead of silently trusted. Python in `authoring_native.py` is fixed first-party backend code staged by the sealed driver.

## Failure and repair semantics

The native batch is `NonAtomicSequence`. A pre-cancelled request is rejected before PlanVault reservation/side effects and leaves the plan reusable; the native E2E verifies that no object is authored. After reservation, cancellation is checked between bounded native operations. On failure/cancellation with uncertain effects, the Blender child is killed, the session is poisoned and the PlanVault attempt becomes UNKNOWN; no refund, hidden retry or rollback claim is made.

Ordinary incremental `Transform` and `MaterialSlots` plans are not repair: both require a fresh source fingerprint, and material-slot mutation requires an owned unshared mesh. Repair is not a second authority path. Only a completed parent **Transform** can create a child repair. The child uses a fresh exact fingerprint, inherits the root convergence budget and may only restore the already-declared transform. Create/remesh/material/rig/bake/delete repair is rejected.

## Shared data and readback

`mesh_instance` shares a mesh datablock and cannot mutate its material slots or modifiers. `mesh_copy` explicitly duplicates the mesh datablock before independent slots/modifiers. Native acceptance checks source/copy datablock identity and shared-instance user counts.

Paged readback cursors are opaque provider-owned state keyed by authenticated Owner. They are single-use and bound to island/domain/native-session/fingerprint. Replay, cross-session use or intervening native drift fails stale instead of turning an incomplete traversal into absence.

Sampled animation/deformation measurement is explicitly bounded to one requested frame and A's exact `media-time::Rate`; rate mismatch fails closed and the prior Blender frame/subframe is restored before returning so observation does not create source drift. Pairwise geometry measurement separates broad and narrow phases. Non-overlapping AABBs can prove pair separation; overlapping AABBs require the bounded world-triangle narrow phase. Object/triangle budget overflow or degenerate narrow-phase inputs produce UNKNOWN. Self-intersection is explicitly not inferred from pairwise object evidence.

## Export and persistence

The existing fixed GLB exporter remains the only exporter. E inventories bounded object/data/material/action/image dependencies before selection. Linked/override dependencies, unsafe instancing, external targets, unbounded shader graphs and unsupported action expansion fail closed. Caller cannot select another operator or arbitrary exporter arguments.

`composition.persist` creates a new `.blend` and never overwrites. `BlendDataLibraries.write` can expand indirect dependencies, so E first checks the managed island closure and still verifies in a fresh Blender process. External sentinels are observed in the native E2E. Linked/read-only content outside the managed/granted boundary remains denied rather than silently copied.

## Evidence boundaries

F PASS requires the trusted native channel and F's evaluator. C authoring receipt admission requires host-owned logical identities plus the real request/descriptor/runtime binding and C's own adapter. Neither proves the later cross-app GLB import activity.

No perfect sandbox, ACID cross-app rollback, exactly-once execution, global filesystem noninterference, aesthetic quality, all-time animation validity or mesh self-intersection certification is claimed. Godot cross-app verification/provenance remain external readiness blockers; exact-SHA E CI is evidence carried by the workflow artifact, not by this source prose.
