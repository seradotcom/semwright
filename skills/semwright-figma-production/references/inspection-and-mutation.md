# Inspection and mutation

The first-party Figma route is:

Semwright Broker → sandboxed Figma driver → authenticated localhost bridge → official Plugin API

with an optional protected credential socket for official REST operations.

Start from document/page/selection inspection, then narrow to the node or semantic subsystem that owns the change. Describe the selected capability to obtain the current schema instead of copying parameter lists into this Skill.

The bridge tracks session generation and observed document revision. Treat stale-generation or revision conflicts as a request to inspect again; do not suppress them or replay an old mutation blindly.

After mutation, inspect the changed node/document state or use `driver.figma.verify.node` when its current schema matches the verification needed.
