# Integration — Project Graph C

## Frozen inputs and ownership
Main baseline: `b736d41b61c4a4146c9e75c16796e251b025e69f`.
Consumed A C0: `26602e4b25929be869d69ef28fef4dd9713180d7`, isolated cherry-pick `63ba119ec7c2577163941b4eae605bb48399a624`.
A C1 inspected at `7ed5b848e4d2e7af235d6166e6f93e0cf0bac90d`; not consumed as an integrated AV dependency. F reviewed P0, but no F native adapter is imported. No D/E/A/B native receipt acceptance is claimed.

C owns its crate, tests, workflow, scripts, documentation and continuity package. Rebuild uses A PlanVault; C has not created a scheduler, general execution engine or private copy of A's common model. Workspace package/lock changes and the additive platform observation change are separate commits. Preserve the native application owners' branches.

## Delivered component API
`semwright-project-graph` provides logical identities/revisions/receipt contracts, ProjectGraph, ProjectAccess, independent Knowledge dimensions, scoped QueryCursors/impact, declared portable manifests, ScopedObserver bookkeeping, a durable external-operation intent ledger and typed rebuild preparation. Optional feature `store` enables the private SQLite GraphStore. The trusted host, not a deserializable client argument, constructs ProjectAccess and registered receipt adapters.

The source defines `RebuildCatalog` and the P1 rebuild handoff separately. Native execution must be delegated through the existing Broker/controller with current descriptor/grant checks and cancellation; a passing catalog test double is not a Broker trace. Do not execute serialized strings or turn a proposal into a permission.

For native read-only/import/readback evidence, use the separate host-only revision admission boundary. `RevisionCandidate` carries LogicalAssetId, fingerprint/equivalence, binding generation, ObservationRef and Coverage but deliberately omits ProjectId, Owner and AssetRevision. A trusted `RevisionAdapter` is registered for the exact ResourceKey + EvidenceSource + method/version; admission binds authenticated Owner, ProjectId, current asset/generation and allocates the AssetRevision. `accept_revision` records an observation only and never creates a production activity. Raw `RevisionRecord` promotion and a public `project.revision.admit` route do not exist. The built-in scoped-file reconcile path uses the same adapter.

## Broker runtime route
The candidate registers bounded `project.*` built-ins through the existing core execution path. They receive the normal schema validation, policy check, audit, execution gate, cancellation/timeout handling and output validation before Project Graph code runs. Read routes require the named `filesystem.read:root` grant. Mutating private graph-state routes additionally require explicit `project.manage`; a filesystem read grant alone cannot create, rebind, tombstone or import project state.

The daemon configures the private graph service under its protected state directory using `platform-services::current_user_principal()`. That principal is derived from the OS user only (`uid` on Unix, user SID on Windows), excludes Broker/logon session identity and is never accepted from request JSON. Session IDs remain ephemeral and bind cursors/request evidence, not durable ownership. The project root boundary fingerprints the resolved root path so symlink retargeting invalidates access.

There is intentionally no `project.receipt.admit` or `project.rebuild.execute` capability. Receipt admission remains a registered trusted-host API. Typed rebuild proposal/reservation remains C/A library functionality until native owners publish a trusted capability-to-preparation mapping and the controller/Broker execution path can be tested without creating another scheduler.

The additive `ScopedRoot::observe_file` API and Linux implementation are separately committed product primitives. Unsupported hosts retain the default Unsupported implementation. Linux tests use disposable files and prove only bounded instance observation; they are not Broker or Blender/Godot/AV E2E.

## Host sequence
The daemon selects protected owner/project storage outside application grants and derives the durable owner from the authenticated OS user. Each request still carries its fresh Broker session and current policy/grant fingerprint. Opening a project verifies the private owner/project header plus the exact resolved configured root boundary. Query visibility is intersected before names/counts are produced. Native resources still require D/E stable resolver/projection contracts; paths or matching names are not native identity.

For a future non-read-only external rebuild operation, the host must transactionally persist PREPARED, then APPLYING before dispatch, call the existing Broker/provider path, and finally persist the admitted receipt plus terminal intent state together. An interrupted APPLYING record is reconciled as UNKNOWN, never retried implicitly. That native dispatch route is not exposed by the current `project.*` command set.

Consumption order is main + the agreed A contract, then C P0/P1, then reviewed native D/E/F/A/B adapters. If A C0 is already in the integrator's main, do not cherry-pick another copy. Reconcile Cargo/workspace history normally; do not copy common sources or reference another agent's worktree from Cargo.

Observed downstream status on 2026-09-28: D model source `568e1edc14528436d49b56dfcfb990756f9a364e` has a green model lane, but its native Godot observation/import adapter remains in development. E source `872aa964a8864d138bac43e6eac46306b498e4a7` exposes a C P0 receipt candidate that keeps durable IDs host-owned, pins descriptor/runtime/plan/parameters/effect contract, and requires C `ReceiptAdapter` admission; its coverage remains UNKNOWN/not cache-safe by construction. E native acceptance is still pending. These are compatibility observations, not C13 completion.

## Persistence and migration
Store schema 1 uses a versioned header, append-only hash-chained journal and materialized asset index. The optional instance-binding event is additive; old records without instance evidence remain unbound. Older readers cannot be assumed to accept new event variants. Unknown schemas fail closed. Explicit index repair only rebuilds from a verified canonical journal. Backups are bounded private snapshots restored into a new directory, not portable sharing manifests. See CONSISTENCY.md for budgets and non-claims.

## Commands and evidence
Run the committed diagnostic selector in GitHub Actions only: `python3 scripts/project-graph/run-suite.py auto`. It verifies the exact checkout SHA. `store` exercises model/store/Skill tests, strict lint, schemas and rustdoc. `full` additionally runs Broker `project.*` policy/owner/session integration tests, the durable OS-principal regression, daemon compile wiring and four bounded fuzz targets. `lockfile` and `fuzz-lock` only resolve dependencies and explicitly do not count as tests. A new fix requires a new commit/run. P0 evidence remains run 36502360629 / job 109195916146 at SHA 6ee52b428310370d3ad438a13964086a63f48367; later evidence is recorded per exact SHA.

The continuity Skill binds real discovery plus registered `project.*` descriptors. Its examples are schema checks and execute zero project operations; the separate Broker integration tests exercise actual project routes with synthetic private roots. Native D/E/A/B continuity and rebuild execution remain separate gates. No PROJECT_GRAPH_READY, native cross-app acceptance, independent R16 closure or merge to main is implied by this component documentation.
