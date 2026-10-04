# Native SDK

The Native SDK lets native applications participate in Semwright without giving up ownership of their model, storage, or transaction boundaries.

Applications expose small, optional cooperation interfaces. The SDK adapts those interfaces to the existing Driver SDK and Driver Host while Broker/Policy, Project Graph, Effect Conformance, jobs, and platform services keep their canonical responsibilities.

## Design

- Application-owned model, persistence, revisions, and transactions.
- Optional file-backed Scene/Table/Counter reference profile.
- TypeScript + SQLite inventory example with CAS, recovery, bounded retention, events, snapshots/workspaces, and private publication.
- Canonical Driver Host execution context and native-reference lifecycle.
- Project Graph and Effect Conformance adapters without duplicate authority.
- Executable TypeScript binding and clean external Rust/TypeScript consumers.
- Exact-SHA CI, native portability coverage, clean-room packaging, and real Host E2E.

## Start here

- `QUICKSTART.md` — minimal integration.
- `API.md` — public API and feature flags.
- `APP_OWNED_STATE.md` — storage and transaction ownership.
- `OPERATIONS_AND_AUTHORITY.md` — execution and authority boundaries.
- `IDENTITY_AND_REVISIONS.md` — durable identity, generations, and opaque revisions.
- `RECOVERY_AND_RETENTION.md` — uncertain results and bounded recovery data.
- `SNAPSHOTS_AND_PUBLICATION.md` — optional snapshot/workspace/publication capabilities.
- `GRAPH_AND_EFFECTS.md` — canonical Graph and Effects integration.
- `BINDINGS.md` — TypeScript bridge.
- `COMPATIBILITY.md` — verified platform coverage.
- `VERIFY.md` — CI and conformance expectations.
- `MIGRATION_FROM_0_3.md` — migration guidance for earlier Native SDK integrations.

The Native SDK follows the repository license: MIT OR Apache-2.0.
