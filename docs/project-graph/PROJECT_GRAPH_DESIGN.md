# Project Graph design — P0

## Problem and observed baseline
Baseline: `b736d41b61c4a4146c9e75c16796e251b025e69f`. This graph answers logical identity, exact production dependencies, invalidation, reusable outputs and remaining unknowns. It preserves native projects. It is not a scheduler, artifact transport, universal app format, vector database or conversational memory.
The existing Broker owns policy, ephemeral refs and provider generations. `artifact.handoff` already copies bounded bytes between named grants; its SHA-256 does not establish logical identity. Workflow Distillation already stores traces and Recipes; C does not modify its private store. `ScopedRoot` and `platform-services::private_directory` provide relevant existing confinement/ownership primitives.

## Consumed shared contract
A C0 `26602e4b25929be869d69ef28fef4dd9713180d7` is imported as an isolated history-preserving cherry-pick. C reuses Owner, ResourceKey, BaseStateSet, ObservationRef, VerificationReport, Digest and semwright-json-v1. That canonicalization is not RFC 8785. A C1 at observed A SHA `7ed5b848e4d2e7af235d6166e6f93e0cf0bac90d` was inspected but is not yet an integrated AV dependency.

## Identities and observations
ProjectId, LogicalAssetId, AssetRevision, DerivationId and ReceiptId are separate opaque namespaces. Two byte-identical files do not share identity by default. A rename preserves an explicitly bound identity; replacement at the same locator does not establish continuity. DurableLocator is only a re-resolution hint: a granted relative file or a versioned native resolver. Historical Owner/base/session values are evidence, never restart credentials. Rebind must be explicit, versioned and audited; ambiguity remains UNKNOWN.
Fingerprint preserves bytes and a separately versioned semantic projection. No mtime-based equivalence. Exact reversion may be reconsidered under the receipt's equivalence policy. Parameters, runtime, descriptor, Recipe, import settings, font, texture, plugin and contract digests are determining dependencies. Incomplete extraction retains an unknown frontier.

## Authority and evidence
A caller cannot promote arbitrary JSON into trusted execution evidence. ExecutionReceipt is bounded wire data. AdmittedReceipt is non-deserializable and is created only by registered trusted host adapter code after a Broker result, binding authenticated owner, request, operation descriptor and runtime. Admission is distinct from verification. Fixture/simulation evidence retains A's UNKNOWN verdict. Product routes must never expose adapter registration/admission to clients.
Relations are a registered enum: contains, references, derived_from, produced_by, consumed_by, realizes, published_as, verified_by. Declared, observed and executed edges stay distinct. Declaring derived_from does not prove a transformation happened. References may cycle; rebuilding production dependencies requires cycle diagnostics, not an invented topological order.

## Knowledge and queries
Existence, freshness, divergence, verification and coverage remain separate. Denied/offline/failed/ambiguous probes imply unknown existence. Only conclusive authorized not-found means MISSING. A known changed input makes a derivative stale even if its own bytes have not changed. Reuse requires complete determining dependencies, current/clean/present state and sufficient verification on the checked scope.
Impact queries must separate known causal paths and possible impact, respect trusted project grants before traversal, carry explicit truncation/cancellation and avoid leaking hidden counts or names. Pagination must bind principal, query and immutable snapshot; mutation cannot silently mix pages. Rebuild is a typed proposal re-entering Broker policy, never stored shell code or an alternate scheduler.

## Persistence and recovery decision boundary
P0 is the model contract, not a completed store. Persistence will preserve immutable revisions/activities and rebuildable materialized indexes, enforce private owner/project storage, bound bytes, and test every commit boundary. The repository has no existing database dependency; a bounded journal with atomic head or an explicitly justified embedded database must reuse the existing trusted state-directory boundary. Local graph atomicity never implies atomic external Godot/Blender mutations. Uncertain external outcome remains pending/UNKNOWN and requires idempotent receipt recovery or scoped reconcile, not blind retries.
Portable imports carry declarations only; imported UUIDs/digests do not acquire authority or CURRENT. Exports omit absolute paths/secrets. GC cannot delete user sources or outputs; destructive management needs an explicit verified scope.

## Ownership and open acceptance
D/E own native projectors and export/import receipts; B owns audio; A owns Composition and AV publication; F owns effect evaluation over A types. C stores their bounded observations, not competing implementations. Blender→GLB→Godot and motion/AV/audio require native receipts on an integrated SHA. P0 tests alone do not satisfy those gates. Broker wiring, durable store, full graph traversal, continuity Skill, native acceptance, scale/fuzz and packaging remain OPEN until separately evidenced. No R16 closure or product-ready claim.
