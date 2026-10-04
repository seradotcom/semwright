# Application-owned state

The Native SDK does not require `document.json`, `NativeApp<Model>`, SQLite or any other persistence engine. `NativeApp<Model>` is the optional file-backed reference profile only.

## Integration pattern

1. Keep the application's existing model/repository and durable logical resource IDs.
2. Expose a bounded observation that returns the native resource generation and opaque revision.
3. Register only supported operations with truthful schemas and guarantees.
4. For a target-bound mutation, compare `CallContext::expected()` with current native state inside the same transaction/lock that commits the change.
5. Commit the application model and app-owned request receipt together only when that is the declared guarantee.
6. Return uncertain completion whenever commit outcome cannot be established.

A preflight observation alone is never an atomic CAS.

## Inventory reference consumer

`examples/native-inventory` owns `supply.sqlite3`, uses SQLite transactions, transports revisions larger than JavaScript's safe integer limit as strings, persists request epochs/receipts/events/snapshots/derivations, and reopens the same database after process restart. Manual UI writes and SDK-mediated writes touch the same resource revisions.

Conformance races two independent Node processes against one base and requires exactly one transaction winner. A manual app edit invalidates an stale SDK base while an independent resource remains unchanged.

Scene/Table/Counter preserve the legacy 0.3 behavior and regression corpus, but applications that do not enable `file-backed` do not inherit that storage model.
