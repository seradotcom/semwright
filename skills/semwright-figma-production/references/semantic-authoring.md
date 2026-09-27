# Semantic authoring

Prefer the high-level composition surface for substantial interface construction when it is available.

The intended flow is:

```text
inspect -> plan -> review ChangeSet -> apply -> measure -> validate -> repair plan -> authorized repair -> reverify
```

A composition plan is bound to document identity, plugin session/generation and revision. It is not authority and must not be edited to broaden scope. If the document changes materially after planning, inspect again and make a new plan.

Express semantic relationships and native Figma intent: stacks, rows, splits, grids, overlays, intrinsic/fill/fixed sizing, minimum gaps, aspect ratios, profiles and design-system references. Use low-level operations only when precision or a construct not modeled by the high-level layer requires them.

Do not turn the Skill into a layout engine. Geometry resolution, ref staleness, constraint enforcement and mutation authorization belong to runtime/driver/Broker.
