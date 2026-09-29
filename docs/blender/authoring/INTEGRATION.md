# E integration map

## Frozen integration inputs

- resumed main / merged GLB baseline: `7a3bae71144bf2c2278b34fc5743e0ceed6dddd1`
- A C0 contract: `26602e4b25929be869d69ef28fef4dd9713180d7`
- C P0 publication: `6ee52b428310370d3ad438a13964086a63f48367`
- F consumed source: `5ed7d0ff8016d76031ec33846f27a237f196835c`
- GLB source head from PR #154: `74671c11dda2133ce6af939896c49cdbb6ba47d5`

E dependency reconciliation commit is `995d856968a5f1bf16739d11ca122825bf2b9fc1`. PR: #175. Exact source SHAs and runs belong in `ACCEPTANCE.json`/CI evidence; this document does not turn a source commit into acceptance.

## Runtime route

`composition.inspect` observes a bounded whole source projection. `composition.inspect.page` gives provider-owned, single-use pagination for objects, bones, actions, curves, keyframes and selected properties; cursor binding includes Owner, native session, domain and source fingerprint.

`composition.plan` builds Blender-domain operations inside A's `PreparedPlan`, pins F's `EffectContract`, and stores the complete canonical plan in A's `PlanVault`. `composition.apply` revalidates the current native base, reserves the attempt before side effects, executes fixed bridge operations, independently reads source state, evaluates F evidence and returns A's `VerificationReport`. Failed/unknown attempts poison the private driver session rather than being silently replayed.

`composition.repair.plan/apply` is transform-only. It requires an already completed parent transform, fresh current fingerprint and the same root PlanVault budget. Root and repair refs are not interchangeable.

`composition.measure` distinguishes source/evaluated state. Pairwise mesh intersection reports an AABB broad phase separately from bounded world-triangle narrow phase. AABB overlap alone never yields an intersection claim; self-intersection remains UNKNOWN.

`mesh_instance` intentionally shares a native mesh datablock and rejects material/modifier writes. `mesh_copy` is the explicit copy-on-write variant and receives an independent datablock before material-slot/modifier mutation.

`composition.persist` creates a new blend library file. `composition.reopen` must run in a fresh driver process for persistence evidence. GLB export remains the single existing `driver.blender.export.glb` implementation from #154, preceded by E's dependency-closure preflight.

## C / F / D boundaries

F evidence is collected by a compiled trusted `EvidenceAdapter`, not imported JSON. The native E2E requires a real F PASS on the same apply.

E provides `graph_receipt_candidate` for C. The native E2E supplies host-owned Project/Asset/Revision IDs, the real Broker request ID, actual apply descriptor digest and driver runtime digest; C's own registered `ReceiptAdapter` performs admission. That authoring receipt does not pretend that later GLB handoff/import activities already exist.

D receives an artifact, never Blender write authority. D SHA `557ad0b…` has internal typed GLB validation, but E found no public Broker-facing D authoring/import capability at that publication; E11 remains blocked until D exposes and natively verifies that route. See `D_GLB_HANDOFF.md`.

## CI and packaging

`.ci/blender-authoring.json` selects an allowlisted suite. `.github/workflows/blender-authoring.yml` verifies the exact checkout SHA, runs model + native Blender 4.5.14, hostile export closure, bounded `cargo-fuzz`, real Skill validation/bundle and deterministic E source backup. Heavy execution is GitHub-hosted only. A new source commit gets a new run; old-SHA reruns do not certify new code.
