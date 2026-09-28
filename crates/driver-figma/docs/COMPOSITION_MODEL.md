# Composition model

`FigmaCompositionSpecV1` is bounded declarative data. It is neither executable code nor a browser-layout language.

## Shape

The spec contains:

- one target page/parent;
- a flat bounded node table with logical IDs and parent relationships;
- semantic relationships;
- explicit responsive profiles;
- requested validators;
- per-session budgets.

The flat table is deliberate: parent cycles and maximum depth are verified before the bridge is called. Unknown fields are rejected by Serde and command schemas.

Supported node intents include frame, section, stack, row, grid, split, overlay, text, shape, media, component instance and semantic region.

## Layout

Containers map to official Figma layout semantics:

- stack -> vertical Auto Layout;
- row/split -> horizontal Auto Layout;
- grid -> Figma grid layout when supported by the node;
- overlay -> explicit non-Auto-Layout container;
- padding/gap/alignment/distribution/wrap -> corresponding native layout properties;
- fill/hug/fixed -> native sizing semantics.

Fill/hug sizing is applied only after the child has been inserted into its actual parent, because Figma sizing behavior depends on Auto Layout context.

Absolute positioning is not the primary model and is used only by existing explicit lower-level operations or an intentionally absolute composition.

## Text

Text intent covers characters, font family/style, size, line height, letter spacing, max lines and fit strategy. The plugin loads the real font and then observes resulting geometry.

Fit strategies are explicit. Silent arbitrary font shrinking is not allowed. Truncation occurs only when requested. Native text is validated separately from visual evidence.

## Design references

A design reference may contain ID, key and/or name. At least one selector is required. When multiple selectors are supplied, all must identify the same object. Name-only resolution must be unique; ambiguity is an error.

Planning uses one bounded design-system snapshot per plan rather than repeatedly rescanning the document per node.

## Relationships

Version 1 supports before/after, alignment/baseline/anchoring declarations, same width/height, centered-in, min/max gap and aspect ratio.

Not every relationship has an automatic compiler or deterministic repair. Unsupported deterministic proof returns incomplete/UNKNOWN evidence rather than PASS.

## Responsive profiles

A profile has a unique name, declared width and root logical ID. The current implementation checks the real root frame width. It does not claim browser-like runtime responsiveness.

## Hard limits

Global limits include:

- composition bytes: 512 KiB;
- nodes: 512;
- depth: 24;
- relationships: 1024;
- profiles: 8;
- asset refs: 64;
- text bytes per node: 65,536;
- validators: 64;
- repair operations: 64;
- convergence iterations: 8;
- convergence mutations: 128.

A spec may declare stricter budgets but cannot raise global limits.
