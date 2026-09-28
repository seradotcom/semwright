# Semantic authoring dogfood demo

The reproducible product fixture lives at `demos/figma-semantic-authoring/`.

It defines a Semwright launch landing with separate 1440 px desktop and 390 px mobile roots, editorial warm-ivory/cobalt/ink direction, native copy, Auto Layout, native CTA instances and variable/style references.

## Purpose

The demo exercises the product contract, not a fake product-UI animation:

1. inspect the live file and bounded design-system context;
2. create only missing approved design primitives through existing explicit Figma capabilities;
3. plan the 124-node composition;
4. review the revision-bound ChangeSet;
5. apply native nodes;
6. measure observed state;
7. validate;
8. surface the real seeded minimum-gap defects;
9. create bounded repair ChangeSets;
10. apply authorized repairs;
11. remeasure/revalidate;
12. export visual verification artifacts;
13. perform a follow-up semantic edit;
14. prove old-plan reuse fails after revision change.

## Seeded failure

Desktop and mobile hero copy intentionally start with Auto Layout gap 16 while declaring a minimum gap of 24. Validation must report a deterministic `declared_spacing` finding derived from observed Auto Layout state. Repair is expected to change that native gap to 24 and a fresh validation must pass that constraint.

No failure may be manufactured by editing screenshots or altering result metadata after the run.

## Design-system bootstrap

`design-system.requirements.json` names the expected variables, text styles and CTA component. `composition.apply` does not silently create these. The runbook must inspect first and use the existing explicit variable/style/component capabilities for missing primitives.

## Live acceptance

Repository/fake runtime evidence is necessary but not sufficient. A final live claim requires an authorized real Figma Design session and recorded editor/plugin/build/commit/capability evidence. If such a session is unavailable, live acceptance remains BLOCKED/UNEXECUTED and the fake-runtime result is not relabeled as live.
