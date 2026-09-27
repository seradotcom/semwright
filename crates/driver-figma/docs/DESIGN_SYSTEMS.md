# Design systems

Authoritative portable format is `semwright-figma-design-system.json`. The Rust model preserves collections, modes, variable resolved types, values and aliases, component variant matrices/properties, and local styles. Alias validation detects missing targets and cycles. CSS custom properties are parsed statically; JavaScript/Tailwind config execution is forbidden.

The plugin exposes current local collection/variable creation and inspection. Import/export is designed as extract → blank model/file → extract → semantic compare, not as a claim of pixel-perfect whole-file reconstruction. Component resolution must return ambiguity rather than silently choosing among same-name candidates.

Semantic authoring snapshots a bounded set of local components/component sets, variable collections/variables and local text/paint/effect/grid styles once per plan. A design reference may use id, key and/or name; when several selectors are supplied they must all identify the same object. Missing primitives are not silently created by `composition.apply`: use the existing explicit component/variable/style capabilities under normal policy.
