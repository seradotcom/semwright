# Migration from Native SDK 0.3

Native SDK 0.3 integrations can migrate incrementally. The current SDK keeps the application's own model and persistence as the source of truth; file-backed storage is an optional compatibility profile rather than a universal requirement.

## Recommended sequence

1. Keep existing application identifiers and persistence.
2. Expose observations using durable resource identity, generation, and opaque revision tokens.
3. Register only operations the application actually supports.
4. Move mutation conflict checks into the same application transaction that commits the effect when claiming atomic CAS.
5. Add recovery only for operations that can persist a durable request/result record.
6. Enable Driver, Graph, Effects, package, or process-bridge features only where needed.
7. Replace direct/in-process execution with Driver Host execution for authority-bearing operations.
8. Validate external consumers against public APIs rather than workspace-internal modules.

Scene, Table, and Counter remain available through the `file-backed` profile for compatibility and examples. Applications with their own database or transaction layer should implement the cooperation interfaces directly instead.
