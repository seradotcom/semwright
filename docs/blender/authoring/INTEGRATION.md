# Blender authoring integration map

## Frozen integration inputs

- resumed main / merged GLB baseline: `7a3bae71144bf2c2278b34fc5743e0ceed6dddd1`
- Composition C0 contract: `26602e4b25929be869d69ef28fef4dd9713180d7`
- Project Graph P0 publication: `6ee52b428310370d3ad438a13964086a63f48367`
- Effect Conformance consumed source: `5ed7d0ff8016d76031ec33846f27a237f196835c`
- GLB source head from PR #154: `74671c11dda2133ce6af939896c49cdbb6ba47d5`

Blender dependency reconciliation commit is `995d856968a5f1bf16739d11ca122825bf2b9fc1`. PR: #175. Exact source SHAs and runs belong in `ACCEPTANCE.json`/CI evidence; this document does not turn a source commit into acceptance.

## Runtime route

`composition.inspect` observes a bounded whole source projection. `composition.inspect.page` gives provider-owned, single-use pagination for objects, bones, actions, curves, keyframes and selected properties; cursor binding includes Owner, native session, domain and source fingerprint.

`composition.plan` builds Blender-domain operations inside the shared `PreparedPlan`, pins the Effect Conformance `EffectContract`, and stores the complete canonical plan in the shared `PlanVault`. `composition.apply` revalidates the current native base, reserves the attempt before side effects, executes fixed bridge operations, independently reads source state, evaluates Effect Conformance evidence and returns the shared `VerificationReport`. Failed/unknown attempts poison the private driver session rather than being silently replayed.

Ordinary incremental `Transform` and `MaterialSlots` intents use fresh source fingerprints and remain normal A/F plans. Material-slot edits require an owned unshared mesh and are rejected for shared `mesh_instance` data. `composition.repair.plan/apply` is separate and transform-only: it requires an already completed parent transform, fresh current fingerprint and the same root PlanVault budget. Root and repair refs are not interchangeable.

Typed relations compile to normal Blender relations: parent/bone parent, world-space follow, fixed-axis look-at and axis-selective `align` via COPY_LOCATION with explicit owner/target spaces. `composition.measure` distinguishes source/evaluated state. Optional sampled animation measurement uses A's exact `media-time::Rate`, evaluates one declared frame, reports exact rational sample time and restores the previous frame/subframe. Pair measurements report origin/center offsets, AABB clearance and contact evidence; mesh intersection separately reports AABB broad phase and bounded world-triangle narrow phase. AABB overlap alone never yields an intersection claim; self-intersection remains UNKNOWN.

`mesh_instance` intentionally shares a native mesh datablock and rejects material/modifier writes. `mesh_copy` is the explicit copy-on-write variant and receives an independent datablock before material-slot/modifier mutation.

`composition.persist` creates a new blend library file. `composition.reopen` must run in a fresh driver process for persistence evidence. GLB export remains the single existing `driver.blender.export.glb` implementation from #154, preceded by the Blender dependency-closure preflight.

## Project Graph / Effect Conformance / Godot boundaries

Effect evidence is collected by a compiled trusted `EvidenceAdapter`, not imported JSON. The native E2E requires a real Effect Conformance PASS on the same apply.

Blender provides `graph_receipt_candidate` for Project Graph. The native E2E supplies host-owned Project/Asset/Revision IDs, the real Broker request ID, actual apply descriptor digest and driver runtime digest; The registered Project Graph `ReceiptAdapter` performs admission. That authoring receipt does not pretend that later GLB handoff/import activities already exist.

Godot receives an artifact, never Blender write authority. The initial `557ad0b…` inspection predated Godot's public Broker-facing cross-app artifact-handoff/import route. That route now exists, but E11 remains blocked until an exact-SHA Godot candidate actually passes native import/readback/semantic verification against an authentic Blender GLB and returns the Godot/Project Graph/Effect Conformance evidence described in `D_GLB_HANDOFF.md`.

## CI and packaging

`.ci/blender-authoring.json` declares **iteration** as the default mode and the exact certification trailer `Semwright-Certify: blender-authoring`. On ordinary pushes, `classify_iteration.py` diffs the pushed SHA against the push base and activates only the affected Blender-authoring areas: model, native, export, security, fuzz, Skill or source-package. Native/export changes still provision pinned Blender 4.5.14, while docs-only or CI-harness-only changes do not consume Blender runners. Iteration artifacts are named `blender-authoring-iteration-<sha>` and are diagnostic only. A candidate intended for integration is represented by an exact commit carrying the certification trailer and setting `.ci/blender-authoring.json.certification_request` to that commit's parent SHA; the classifier rejects any mismatch. That marker-only commit therefore certifies the already-iterated parent without mixing fresh product changes into certification. Certification mode forces every area gate, preserves the historical final artifact name, runs model + native + startup security + fresh-Blender roundtrip + hostile export + bounded fuzz + Skill bundle + deterministic source backup + formatting, and only then permits `finalize_evidence.py` to emit `acceptance-final.json`. Heavy execution is GitHub-hosted only, and iteration evidence can never be mistaken for certification evidence.
