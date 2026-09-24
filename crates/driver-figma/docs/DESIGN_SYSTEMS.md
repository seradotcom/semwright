# Design systems

Authoritative portable format is `semwright-figma-design-system.json`. The Rust model preserves collections, modes, variable resolved types, values and aliases, component variant matrices/properties, and local styles. Alias validation detects missing targets and cycles. CSS custom properties are parsed statically; JavaScript/Tailwind config execution is forbidden.

The plugin exposes current local collection/variable creation and inspection. Import/export is designed as extract → blank model/file → extract → semantic compare, not as a claim of pixel-perfect whole-file reconstruction. Component resolution must return ambiguity rather than silently choosing among same-name candidates.
