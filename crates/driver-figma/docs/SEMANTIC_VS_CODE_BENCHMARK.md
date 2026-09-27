# Semantic authoring vs code-first benchmark

The benchmark asks whether semantic authoring materially improves creation and continued manipulation of a polished native editable Figma document. It is not designed to force a semantic win.

The machine-readable configuration is `demos/figma-semantic-authoring/benchmark-config.json`.

## Arms

### A — code-first

Use HTML/CSS/SVG or another legitimate code-oriented representation with the same brief/assets/iteration budget. If transferred into Figma, use a legitimate existing route. Do not artificially forbid techniques that make code-first effective.

### B — existing low-level Figma path

Use the pre-existing `driver.figma.*` primitives but not `composition.*`. The agent is responsible for the low-level orchestration.

### C — semantic authoring

Use bounded design-system inspection plus `composition.plan/apply/measure/validate/repair/verify`.

## Metrics

Record only measurable evidence: native editable TextNode percentage, native component use, variable/style binding, Auto Layout correctness, semantic structure, giant SVG count, clipping/overlap/out-of-bounds defects, mobile correctness, repair rounds, tool calls, manual interventions, unresolved findings, post-hoc semantic editability and specific-node verification.

Visual quality is an AESTHETIC_ASSIST comparison. No objective beauty score is produced.

Unavailable wall-time/token/model metrics are recorded as UNKNOWN/UNEXECUTED rather than estimated.

## Follow-up mutation

After initial completion, all arms receive the same follow-up request to modify a design-system or component-level property. The result is inspected again to measure whether the output remains semantically controllable rather than merely visually similar.

## Current evidence rule

Fake-Figma and repository tests can prove protocol, native-structure intent, stale-revision rejection, deterministic validation and repair mechanics. They cannot prove live Figma rendering quality or a visual win against code-first.

Therefore the human-readable final verdict must distinguish:
- automated structural evidence;
- live Figma evidence;
- visual benchmark evidence.

If a live/code-first arm cannot be executed under comparable conditions, no winner is declared for that dimension.
