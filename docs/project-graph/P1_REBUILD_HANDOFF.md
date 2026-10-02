# Project Graph P1: rebuild proposals and existing-vault binding

## Compatibility and ownership

This additive contract consumes A C0 `26602e4b25929be869d69ef28fef4dd9713180d7` from the pinned C branch, not copied A sources. P0 receipt, identity and evidence schemas remain version 1. New rebuild schemas have their own version-1 proposal envelope. There is no storage migration from this module. `observe_determinants` now requires full-project visibility because it replaces the entire observed determinant set; a subset grant cannot replace hidden dependencies.

C owns `RebuildRequest`, `RebuildProposal`, `RebuildBinding`, `RebuildReservation` and the reconstruction query. A owns `PlanVault`, `ConvergenceBudget`, controller, native prepared plans and evidence. D/E own native preparation/realization and identity resolvers; B owns audio and A AV publication; F owns effect semantics. Nothing here claims that their native adapters have been consumed or tested.

## Read-only proposal

`ProjectGraph::propose_rebuild(access, targets, catalog, budget, cancellation)` walks active production receipts backwards from requested outputs. It returns exact historical input/output pins, required upstream production activities, a current registered preparation binding, blockers and reusable outputs. It does not store application arguments, shell commands, executable code or a native session ref.

Only outputs satisfying existing `Knowledge::cache_safe()` are reusable: present, current, clean, required verification PASS, complete determining dependencies, and no reconcile requirement. An observed source without a producer may be an input, but is not called an output-cache hit. A missing/unobserved source, hidden dependency, partial extractor, unavailable capability or divergent output blocks automatic preparation. Divergence is not permission to overwrite an external edit.

Production SCCs are computed iteratively and reported explicitly; a cycle yields no topological order. Ordinary reference/containment edges are not invented production dependencies. In-place production is also explicitly blocked. Acyclic plans have deterministic dependency-first ordering. Cancellation or node/edge/depth/result exhaustion returns explicit partial/unknown state and no complete ordering. Canonical wire-size limits remain enforced by A; oversize proposals fail rather than returning an unlabelled subset.

## Catalog adapter

`RebuildCatalog::lookup` is implemented by trusted host code reading a `RebuildCatalogSnapshot` produced by Core. Registry preparation relations are registered only through the host API, never provider aliases/tags/object types, and pin both descriptor digests under one provider/version. Core binds the active provider generation/connection into the current runtime digest. Provider refresh drops the relation; explicit host re-registration is required and yields a new runtime binding. Never treat client JSON or a saved `RebuildBinding` as catalog evidence.

## Reservation and preparation

`reserve_rebuild` recomputes the proposal from targets and current graph state, rejects blocked/empty work, and binds its complete canonical bytes in the caller's existing A `PlanVault`. The reservation includes graph project/snapshot/observation epoch and actual grant/visibility fingerprints. It is ephemeral; importing JSON or restarting the graph cannot reacquire its authority.

`begin_rebuild_preparation` validates those server-held bytes, owner/session, grants, visibility, snapshot and epoch, checks current catalog bindings again, checks cancellation and then consumes A's `BeginPermit`. Recomputing a client digest, clearing blockers, changing historical parameters, substituting request identity or replaying a consumed reservation does not update the private vault. Expiration and attempt accounting remain A's implementation.

This permit is NOT a policy grant. It only bounds entry to native preparation via the existing Broker/controller. It does not apply the reconstruction, execute an old native plan, authorize exports, clear uncertain effects, or prove external files did not change between observations. Native preparation must reacquire references and current base states. Every actual app operation must re-enter Broker policy; the controller records partial/cancelled/unknown outcomes in A's vault and produces trusted C receipts afterward. The reservation's operation count is preparation calls, not a claim of N native suboperations or N Broker approvals.

There is no scheduler or background process in this module. Replanning after conflict is explicit. C provides a durable ExternalIntent ledger for host integration: PREPARED and APPLYING are store transitions, interrupted APPLYING becomes UNKNOWN after restart, and COMPLETED requires a matching admitted receipt. Core's host-only `execute_rebuild_preparation` revalidates the current relation and then enters the ordinary Broker; it does not make the proposal a permission and it does not dispatch a sequence on its own. The host/controller still owns PREPARED→APPLYING→terminal transitions and receipt persistence around each actual external operation.

## Contract tests and CI

`tests/rebuild.rs` covers dependency ordering, exact cache reuse versus restart UNKNOWN, production SCCs, hidden receipts, subset determinant writes, cancellation, node/edge budgets, unavailable catalogs, external-output divergence, canonical-plan substitution, session/grant/snapshot/epoch binding, runtime drift and replay rejection.

`tests/rebuild_differential.rs` additionally compares production SCCs against independent pairwise reachability over 48 generated graphs, including self-cycles and disconnected components. It does not call the production SCC helper.

These tests use synthetic host-side model observations and are NOT Blender/Godot/AV native evidence. The diagnostic script inventories and runs all crate tests on the exact pushed SHA; a queued run, passing test step followed by failed Clippy, or the earlier P0 PASS is not a P1 release PASS. The normal source lane is `cargo test --locked -p semwright-project-graph --features store --all-targets`, in GitHub Actions only, followed by strict Clippy, generated schemas and rustdoc.

## Runtime integration boundary

The candidate derives `ProjectAccess` from daemon-owned OS identity plus current Broker session/grants for registered graph-state routes. Rebuild proposal/reservation remains intentionally unexposed. Native owners/integrators explicitly register their production→preparation relationship with Core, use C's snapshot as the `RebuildCatalog`, consume A's vault permit, and dispatch the one preparation capability through `execute_rebuild_preparation`. The helper revalidates the binding immediately before ordinary Broker execution; it is not a generic execute-plan command or second engine. Blender→GLB→Godot is externally certified at D `7eaf8de...`; motion/AV/audio still requires one A+B combined SHA.
