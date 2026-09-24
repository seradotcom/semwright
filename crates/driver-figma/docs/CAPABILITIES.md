# Capability catalog

The semantic-completeness branch advertises 393 bounded `driver.figma.*` capabilities:

- 334 operations backed by the typed official Plugin API dispatcher;
- 56 cloud operations: 54 pinned official Figma REST endpoints, one documented semantic discovery helper, plus `cloud.status`;
- 3 local driver/session operations: doctor, pairing and session inspection.

The catalog covers document/page/selection, scene-node inspection and mutation, generic property semantics, declarative compose/batch creation, layout, typography, vector/image/media/embed handling, components/variants/instances/Slots, variables/modes/bindings, styles/libraries/shaders, snapshots/diffs, validation, prototyping, Motion, viewport state, FigJam, Slides, Buzz, Dev Mode/codegen/text review, artifact-backed exports and cloud collaboration/administration APIs.

Every advertised capability must have a non-placeholder input/output schema and one of: a Plugin API handler, a local Rust implementation, or an allowlisted REST implementation. `catalog_consistency.py` fails on descriptor/handler drift and rejects production escape hatches such as eval, Function constructors, app.asar patching or remote-debugging-port control.

`API_COVERAGE.json` is the machine-readable Plugin API exhaustiveness record. It inventories the pinned typings rather than multiplying every property into a separate tool. Generic property read/write operations use a generated allowlist with per-member mutability/type/policy metadata.

`REST_API_COVERAGE.json` records every pinned OpenAPI operation, scope, credential class and semantic capability mapping. Cloud credentials are not capability arguments and are not returned to the agent.

Session-specific availability can still depend on editor type, manifest permissions, Motion Beta, team/library access, plan tier or document state. Those conditions fail explicitly at execution time; they are not hidden by arbitrary JavaScript.
