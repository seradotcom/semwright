# Semwright Figma semantic-authoring dogfood

This package is the reproducible product fixture for the semantic-authoring mission.

Files:

- `landing.spec.json` — 124-node desktop/mobile `FigmaCompositionSpecV1`.
- `design-system.requirements.json` — named primitives that must exist or be created through existing explicit capabilities.
- `expected-checks.json` — semantic acceptance conditions and the seeded real gap defects.
- `benchmark-config.json` — common three-arm benchmark contract.
- `design-brief.md` — same creative brief for every arm.
- `runbook.md` — exact Semwright production sequence.
- `model-instructions.md` — model instructions used for dogfood.
- `benchmark-results.json` — live/product evidence ledger; UNKNOWN/UNEXECUTED is preserved instead of guessed.
- `build_demo_spec.py` + `semwright-landing.composition.json` — deterministic 73-node secondary fixture used only for fake-runtime structural benchmarking.
- `convergence_demo.py` — real Driver Protocol plan/apply/validate/repair/reverify/edit/stale-plan acceptance over the repository fake runtime.
- `benchmark.py` + `structural-benchmark-config.json` — reproducible A/B/C structural comparison (code-first SVG import vs low-level native primitives vs semantic authoring). Visual quality remains explicitly UNEXECUTED in fake Figma.
- `STRUCTURAL_BENCHMARK.md` — scope and limitations of that secondary benchmark.

The 124-node `landing.spec.json` remains the primary product/live-Figma fixture because it exercises components, variables/styles and richer design-system semantics. The 73-node generated fixture exists to make protocol-level benchmark runs deterministic; it does not replace the product demo.

The package contains no credentials, user sessions, private Figma documents or remotely fetched assets.
