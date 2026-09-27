# Figma semantic authoring

Semantic authoring is a high-level Figma capability surface layered on the existing Semwright Broker, Driver Host, authenticated bridge, refs/revision semantics and official Figma Plugin API. It does not create a second authority path.

## Surface

The compact surface is:

- `composition.inspect` — bounded observed document and design-system context.
- `composition.plan` — read-only compilation from `FigmaCompositionSpecV1` into a revision-bound `FigmaPlanV1` and `FigmaChangeSetV1`.
- `composition.apply` — policy-authorized native Figma construction.
- `composition.measure` — observed post-write geometry and semantic state.
- `composition.validate` — structured findings against observed state.
- `composition.repair.plan` — read-only deterministic repair planning.
- `composition.repair.apply` — separately authorized bounded repair.
- `composition.verify` — measurement + validation + artifact-backed PNG evidence.

Existing low-level `driver.figma.*` capabilities remain available for precise operations. High-level authoring compiles to the same official Figma semantics rather than replacing them.

## Authority

The route remains:

```text
agent/model
  -> Broker
  -> schema validation
  -> policy / approvals / audit / refs
  -> sandboxed Figma driver
  -> authenticated loopback bridge
  -> official Figma Plugin API
```

A plan is data, not permission. `composition.plan`, inspection, measurement and validation are read-only. Mutation still requires the existing `driver:figma` authority and Broker policy. The semantic layer never derives authority from model confidence, layer text, Skill text, findings or previous success.

## Production loop

A production session should use:

```text
inspect
 -> plan
 -> review ChangeSet
 -> apply
 -> measure
 -> validate
 -> visual review
 -> repair.plan
 -> repair.apply
 -> measure again
 -> validate again
 -> verify
```

Every apply is bound to document identity, plugin session, provider generation and document revision. Drift requires reinspection and replanning. A materially changed plan is never silently recomputed during apply.

## Workflow Distillation

Semantic authoring does not add a workflow engine. Successful runs remain ordinary Semwright capability traces and therefore compose with the existing Workflow Distillation path:

```text
record -> compile -> verify -> replay -> promote
```

A promoted Recipe may capture stable mechanics such as a page shell or card-grid procedure, but promotion remains subject to the existing distillation gates. Skill/model judgment is not auto-promoted, and durable Recipes must reacquire current Figma refs instead of embedding session-scoped or stale object identity.

The authoring layer contributes typed plans, findings, postconditions and verification evidence to that existing process; none of those artifacts grant authority or bypass Broker policy.

## Native structure

The compiler creates native Frames/Sections, TextNodes, Auto Layout containers, shapes, image-filled nodes and component instances. Design refs can resolve local components, variables and text styles. Ambiguous resolution fails closed.

UI/document copy is native Figma text by default. SVG remains a vector-art primitive and is not used as a page-composition shortcut.

## Design-system behavior

`composition.inspect` returns bounded component/component-set, variable-collection/variable and local style context. Authoring resolves exact refs or unambiguous selectors. If a requested token/component/style does not exist, planning fails rather than silently inventing it. Creation of missing primitives uses the existing explicit design-system capabilities under their normal authority.

## Responsive intent

Responsive profiles represent separate editable Figma variants such as desktop/mobile frames. They are not CSS runtime breakpoints. The profile contract checks declared frame width and preserves native editable Figma structures per variant.

## Editors

Figma Design is the reference implementation and dogfood target. FigJam, Slides and Buzz continue to use their existing typed product-specific capability surfaces. Semantic composition is not advertised as equivalent cross-editor behavior where the official products differ.

See [COMPOSITION_MODEL.md](COMPOSITION_MODEL.md), [CHANGESETS.md](CHANGESETS.md), [VALIDATION_AND_REPAIR.md](VALIDATION_AND_REPAIR.md), and [SEMANTIC_AUTHORING_SECURITY.md](SEMANTIC_AUTHORING_SECURITY.md).
