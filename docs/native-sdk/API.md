# Native SDK API

## Base cooperation contract

`semwright_native_sdk::cooperation` exposes opaque `RevisionToken` / `ResourceVersion`, revision-bound `Query` / `PageCursor` / `ObservationPage`, explicit `OperationContract`, `RequestIdentity` / `RecoveryRecord`, `Completion`, `CallContext`, `Application`, and shared value/digest helpers.

`Application` is a composition helper, not a model or repository. An observation-only app is valid. Domain capability groups such as snapshots, workspaces, event polling and private publication are exposed by the presence of real registered canonical operation descriptors; absence is legitimate and is not reported as synthetic success.

## Operation guarantees

`OperationContract` keeps canonical `CommandDescriptor` risk/idempotency and separately declares commit, retry, undo, cancellation, target and atomic revision-CAS semantics. A mutation is not automatically reversible or idempotent. `atomic_revision_cas` is valid only when comparison and commit occur inside the same application transaction.

## Feature flags

| Feature | Purpose | Required universally? |
|---|---|---|
| none | portable cooperation values/contracts | base only |
| `driver` | canonical Driver SDK adapter | no |
| `process-bridge` | owner-pinned Host-mediated Node bridge | no |
| `file-backed` | Scene/Table/Counter reference storage profile | no |
| `effects` | canonical Effect Conformance integration | no |
| `graph` | canonical Project Graph candidate/locator adapter | no |
| `package` | operator-side package lifecycle | no |
| `host-tests` | private conformance wiring | testing only |

## Errors and uncertainty

The SDK reuses canonical `semwright-types::Error` / `ErrorCode`. A malformed or lost reply after a mutation can be uncertain; it must not be rewritten into a known rejection. Historical recovery does not create current permission. Unsupported interfaces are absent or return canonical `Unsupported`; declared-but-broken interfaces fail conformance.
