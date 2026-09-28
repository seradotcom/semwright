# Figma ChangeSets

Figma semantic authoring uses `FigmaChangeSetV1` to make intended mutation inspectable before execution. This is a domain ChangeSet, not a claim of ACID transactions.

## Plan binding

`FigmaPlanV1` binds:

- document identity;
- authenticated session;
- provider generation;
- observed revision;
- validated composition spec;
- resolved creates/modifies;
- required scope;
- risk;
- validators/postconditions;
- SHA-256 digest.

Apply verifies the digest and the current document/session/generation/revision. Any mismatch fails with a conflict/stale-reference error.

## Scope and risk

Version 1 ChangeSets require exactly the existing `driver:figma` scope and `mutating_reversible` risk. A ChangeSet cannot request a broader composition scope, a repair bypass, or a superuser capability.

Deletes are deliberately forbidden inside semantic ChangeSets. Destructive operations remain on explicit existing capabilities with their existing policy treatment.

## Semantic changes

Creation entries use logical identity and resolved design bindings. Repair modifications use an allowlisted action such as:

- grow native text height;
- restore an explicit Auto Layout gap;
- restore an explicit aspect ratio;
- bind an explicitly required fill-color variable.

The intent is preserved as semantic data rather than collapsing every change to anonymous x/y coordinates.

## No false atomicity

Figma undo grouping and Semwright ChangeSets are not generic atomic transactions. A failed mutation can have partial or uncertain effects if the underlying official API/transport cannot prove otherwise. Semwright reports those outcomes honestly and requires fresh observation after mutation.

There is no automatic rollback guarantee in this layer.
