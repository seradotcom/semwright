# Security and repair

Managed authoring is grant-bounded. The agent supplies logical project intent, not arbitrary filesystem paths, executables, scripts, plugins, or host methods.

The provider refuses implicit adoption of a non-empty unowned project directory and rejects traversal, symlink/hardlink escape, stale prepared state, and managed-source drift.

Repair is narrower than regeneration:

- **missing managed source**: eligible for deterministic repair after validation and an explicit repair plan;
- **DIVERGED managed source**: do not overwrite automatically;
- **partial publication**: reconcile explicitly before further mutation;
- **human/unmanaged nodes or files**: preserve unless a separate authorized operation explicitly owns them;
- **logic, silhouette, or aesthetic ambiguity**: return to the model/human rather than inventing a deterministic repair.

Behavior budgets limit generated runtime behavior but do not sandbox arbitrary external Godot code. Untrusted projects, addons, resources, and callbacks require disposable native isolation.
