# Semantic authoring implementation note

Frozen inputs:

- `BASELINE_SHA=a04f664969ac830e3cb5a96c6e62e8eefc40b5fe`
- `SKILLS_REFERENCE_SHA=9dffd4d12c78bd85bdb2153b2f61864bdecac0ee`
- Baseline catalog check: 393 advertised / 334 Plugin handlers / 3 local / 56 REST.

Implementation direction:

1. Keep Broker + `driver:figma` as the only authority boundary.
2. Add a bounded flat composition model instead of recursive executable layout code.
3. Bind plans to document identity, session, generation and revision in the Rust driver.
4. Compile semantic nodes and relationships into native Figma nodes, TextNodes and Auto Layout.
5. Persist only non-authoritative semantic metadata needed for measurement/validation.
6. Measure post-write state from the Plugin API; requested geometry is never treated as observed truth.
7. Return structured findings with deterministic / heuristic / aesthetic-assist evidence classes.
8. Generate repair ChangeSets only for an allowlisted deterministic repair set.
9. Reuse `verify.node` artifact semantics for visual evidence.
10. Keep the `semwright-figma-production` Skill aligned with the semantic authoring contract.

Non-goals: arbitrary JS/TS, code interpolation, shell/network/filesystem escape, giant SVG composition, private APIs, fake ACID rollback, implicit repair authority, or a second workflow engine.

