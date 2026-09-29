# Integration — Project Graph C

## Frozen inputs and ownership
Main baseline: `b736d41b61c4a4146c9e75c16796e251b025e69f`.
Consumed A C0: `26602e4b25929be869d69ef28fef4dd9713180d7`, isolated cherry-pick `63ba119ec7c2577163941b4eae605bb48399a624`.
A C1 inspected at `7ed5b848e4d2e7af235d6166e6f93e0cf0bac90d`; not consumed as an integrated AV dependency. F reviewed P0, but no F native adapter is imported. No D/E/A/B native receipt acceptance is claimed.

C owns its crate, tests, workflow, scripts, documentation and continuity package. Rebuild uses A PlanVault; C has not created a scheduler, general execution engine or private copy of A's common model. Workspace package/lock changes and the additive platform observation change are separate commits. Preserve the native application owners' branches.

## Delivered component API
`semwright-project-graph` provides logical identities/revisions/receipt contracts, ProjectGraph, ProjectAccess, independent Knowledge dimensions, scoped QueryCursors/impact, declared portable manifests, ScopedObserver bookkeeping, a durable external-operation intent ledger and typed rebuild preparation. Optional feature `store` enables the private SQLite GraphStore. The trusted host, not a deserializable client argument, constructs ProjectAccess and registered receipt adapters.

The source defines `RebuildCatalog` and the P1 rebuild handoff separately. Native execution must be delegated through the existing Broker/controller with current descriptor/grant checks and cancellation; a passing catalog test double is not a Broker trace. Do not execute serialized strings or turn a proposal into a permission.

## Runtime gate — currently BLOCKED
`integration-drafts/project_graph.rs` is an unwired review draft. The attempted command wiring dependencies, core fields/dispatch and daemon initialization was rejected by the remote tool. Read-only verification confirmed those global changes were not applied. The draft is not compiled, registered or a supported CLI/MCP route. This is a concrete integration blocker, not native acceptance or a delegated bypass instruction.

There is also an identity boundary to resolve before wiring: Broker session IDs are intentionally ephemeral. Unix local IPC authenticates the peer UID before entering `connection()`, but that durable peer identity is not currently propagated into Broker execution context; Windows validates the named-pipe peer process but exposes no shared durable principal abstraction to C. C will not substitute session tickets, request IDs, process IDs or project paths for a durable principal merely to make the adapter compile.

The additive `ScopedRoot::observe_file` API and Linux implementation are separately committed product primitives. Unsupported hosts retain the default Unsupported implementation. Linux tests use disposable files and prove only bounded instance observation; they are not Broker or Blender/Godot/AV E2E.

## Host sequence when integration is permitted and reviewed
A trusted host chooses protected owner/project storage outside all application grants, authenticates the principal independently of request JSON, opens GraphStore with the exact owner/project, and resets live observation state on restart. It intersects query/resource visibility with current grants and registers only trusted backend observers and receipt adapters. Native resources require D/E's stable resolver/projection contracts; paths or matching names are not resolvers. For a non-read-only external operation the host must transactionally persist PREPARED, then APPLYING before dispatch, call the existing Broker/provider path, and finally persist the admitted receipt plus terminal intent state together. An interrupted APPLYING record is reconciled as UNKNOWN, never retried implicitly.

Consumption order is main + the agreed A contract, then C P0/P1, then reviewed native D/E/F/A/B adapters. If A C0 is already in the integrator's main, do not cherry-pick another copy. Reconcile Cargo/workspace history normally; do not copy common sources or reference another agent's worktree from Cargo.

Observed downstream status on 2026-09-28: D model source `568e1edc14528436d49b56dfcfb990756f9a364e` has a green model lane, but its native Godot observation/import adapter remains in development. E source `872aa964a8864d138bac43e6eac46306b498e4a7` exposes a C P0 receipt candidate that keeps durable IDs host-owned, pins descriptor/runtime/plan/parameters/effect contract, and requires C `ReceiptAdapter` admission; its coverage remains UNKNOWN/not cache-safe by construction. E native acceptance is still pending. These are compatibility observations, not C13 completion.

## Persistence and migration
Store schema 1 uses a versioned header, append-only hash-chained journal and materialized asset index. The optional instance-binding event is additive; old records without instance evidence remain unbound. Older readers cannot be assumed to accept new event variants. Unknown schemas fail closed. Explicit index repair only rebuilds from a verified canonical journal. Backups are bounded private snapshots restored into a new directory, not portable sharing manifests. See CONSISTENCY.md for budgets and non-claims.

## Commands and evidence
Run the committed diagnostic selector in GitHub Actions only: `python3 scripts/project-graph/run-suite.py auto`. It verifies the exact checkout SHA. `store` exercises model/store/Skill tests, strict lint, schemas and rustdoc; `full` adds four bounded fuzz targets. `lockfile` and `fuzz-lock` only resolve dependencies and explicitly do not count as tests. A new fix requires a new commit/run. P0 evidence: run 36502360629, job 109195916146, SHA 6ee52b428310370d3ad438a13964086a63f48367. Later test and job conclusions remain separate in the evidence ledger.

The continuity Skill is packaged and validated with the real Skills library and current discovery descriptors. Its examples execute zero project operations. Operational continuity and C15 remain blocked until the actual graph routes and native workflow are available. No PROJECT_GRAPH_READY, native end-to-end support, independent R16 closure or merge to main is implied by this component documentation.
