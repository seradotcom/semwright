# Native SDK quickstart

The application keeps its own model, storage, and transaction boundary. The base Rust crate has `default = []`; it does not require a file store, database, daemon, Graph implementation, or runtime bridge.

## Minimal Rust consumer

Use `semwright_native_sdk::cooperation::{Application, ResourceVersion, RevisionToken}`.

`Application::new("my-application", "1.0.0")` creates a cooperation surface only. Add an `ObservationProvider`, optional `RecoveryProvider`, and the `OperationContract` / `OperationHandler` pairs the application actually supports.

Enable feature `driver` when exposing that cooperation through the canonical Driver SDK. Native authority-bearing operations are executed with a `DriverExecutionContext` issued by Driver Host.

## Reference profiles

- Scene, Table, and Counter use the optional `file-backed` profile.
- Inventory is a separate TypeScript/SQLite application that owns its schema and transactions.
- `graph`, `effects`, `package`, and `process-bridge` are independent opt-in features.
- Consumers use the same public cooperation and Graph locator APIs without adopting an SDK-defined storage model.

## TypeScript

`@semwright/native-sdk` provides bounded JSON values, opaque revisions, request/recovery helpers, and the Host-mediated process bridge.

See `API.md`, `APP_OWNED_STATE.md`, `OPERATIONS_AND_AUTHORITY.md`, and `VERIFY.md` before implementing a provider.
