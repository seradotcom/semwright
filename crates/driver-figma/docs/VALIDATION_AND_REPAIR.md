# Validation and bounded repair

Semantic validation reads actual Figma state after authoring. Requested values are not accepted as proof that the write succeeded.

## Finding model

Findings carry category, severity, evidence class, subject identity, related nodes, expected/actual values, evidence and optional repair candidates.

Evidence classes are:

- `DETERMINISTIC` — machine-verifiable state;
- `HEURISTIC` — useful non-objective signal;
- `AESTHETIC_ASSIST` — judgment for a model/human.

Only deterministic evidence can justify automatic repair, and a finding never grants authority.

## Deterministic checks

The semantic validator currently checks or integrates evidence for:

- text clipping where Figma text-range geometry is available;
- missing fonts;
- parent-bounds overflow;
- accidental sibling overlap in non-overlay manual containers;
- declared min/max spacing;
- same width / same height;
- centered-in and relative order;
- aspect ratio;
- minimum touch target for declared control roles;
- required Auto Layout mode;
- responsive profile width;
- native TextNode requirement;
- component-instance requirement;
- required variable-bound fills.

Existing `validate.lint`, `validate.a11y`, `prototype.validate` and `verify.node` remain canonical for their existing domains. Semantic authoring composes with them rather than copying every rule into a second validator.

Alignment/baseline/anchored relationships that cannot yet be proved from the bounded semantic evidence increase UNKNOWN rather than producing a false PASS. Contrast/prototype/Motion checks continue through the existing specialized surfaces when requested by the production workflow.

## Repair boundary

Automatic repair is limited to an unambiguous single deterministic repair candidate and the source plan's budgets. Current allowlisted repair mechanics are native text-height growth, Auto Layout gap restoration, aspect-ratio restoration and explicit fill-variable binding.

Artistic collisions, hierarchy, visual dominance and similar choices remain model/human judgment.

## Convergence

The convergence loop is intentionally composed from separate capabilities instead of a privileged do-everything command. A controller/agent may iterate within the plan budgets:

```text
fresh measure
 -> validate
 -> repair.plan
 -> separately authorized repair.apply
 -> fresh measure
 -> validate
```

Stop on stale revision/generation/ref, ambiguous repair, policy denial, budget exhaustion, unexpected severity worsening, UNKNOWN evidence that blocks proof, or no deterministic progress.

Every mutation remains a normal Broker-authorized operation and therefore auditable.
