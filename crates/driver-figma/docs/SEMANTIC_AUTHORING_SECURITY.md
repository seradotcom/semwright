# Semantic authoring security

Semantic authoring preserves the existing Broker and Figma Driver trust boundaries.

## Declarative input

Composition specs and plans are data. Unknown fields are rejected. Arbitrary JavaScript/TypeScript, function strings, eval, shell expressions, code interpolation and hidden URL fetches are not part of the model.

The Rust layer validates byte budgets, node/depth/relationship/profile/asset/text/repair/iteration limits, finite numbers, IDs, design refs, relationship parameters, scope/risk and ChangeSet parent graphs before mutation.

## Untrusted document content

Layer names, text, plugin data, comments and design-system names are untrusted data. They can participate in bounded exact/semantic lookup, but they cannot become instructions or policy. Design-ref ambiguity produces an error rather than a silent choice.

Skill text and document text are never execution principals.

## State binding

Plans include document ID, session, generation and revision. The digest covers the entire plan except its digest field. Apply verifies all of these before calling the plugin. Collaborative drift requires reinspection/replanning.

## Authority

Planning, inspection, measurement and validation do not grant mutation authority. Repair suggestions do not grant mutation authority. All writes continue through Broker policy with the existing `driver:figma` scope.

The layer introduces no `figma.superuser`, `composition.all` or repair bypass.

## Network, files and secrets

Semantic authoring adds no generic network or filesystem access. Media references use existing Figma image hashes/artifact routes. It does not accept arbitrary internet URLs.

OAuth/PAT/payment credentials are never composition data. Existing protected REST credential handling remains separate.

## SVG

Whole-page SVG composition is not part of semantic authoring. Existing SVG import remains subject to the driver's sanitizer/restrictions and is appropriate for vector artwork, icons and illustrations—not as a way to hide UI text or flatten a design.

## Resource exhaustion

The global authoring limits cannot be raised by user-declared budgets. Design-system discovery is bounded and snapshotted once per plan. Validation walks a bounded subtree and reports truncation/incompleteness instead of implying full-document PASS.

## Outcome semantics

No ACID or automatic rollback claim is made. Unknown or partial effects remain unknown/partial until fresh observation proves a final state.
