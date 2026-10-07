# Effects and driver conformance

## Product contracts
- EFFECT_GAP_ANALYSIS.md maps shared Composition semantics to additive Effect Conformance work.
- EFFECT_SEMANTICS.md defines intention, authorization, obligations, evidence and verdicts.
- E0_ADAPTER_CONTRACT.md defines the trusted producer boundary.
- DRIVER_QUALITY_CONTRACT.md defines nine workflow-scoped quality dimensions.
- ACCEPTANCE.md and INTEGRATION.md carry requirement and consumer status.

The implementation is `crates/effect-conformance`. Runtime dependency direction is effect-conformance → semantic-composition. Project Graph is dev-only. Effect Conformance does not own Composition reports, PlanVault, canonicalization or policy authority.

## Tested implementation
Implementation source `d2cfd86a2ee064aa5de8f0a8944319edf6dbb060` passed the Effect Conformance release run 36942492444 on Ubuntu/Windows/macOS. Linux additionally executed pinned Godot 4.7.2 and Blender 4.5.14 native conformance plus targeted mutations.
The Linux Effect Conformance artifact requested/executed 40/40 tests with zero skips, including the explicit Forbidden-obligation regression, killed all three targeted mutants and generated bounded native receipts/quality reports. Current-SHA Quality 36942497243, Packaging 36942497427 and Supply-chain 36942497317 are SUCCESS. Dependency/coverage/fuzz 36942497359 has coverage/fuzz PASS but its audit job is red because the current RustSec database marks unchanged `yoke-derive 0.8.3` as yanked.

## Reconstructive backup
`backup/semwright-effect-conformance-d2cfd86.zip` is the CI-produced source backup for the tested implementation SHA.
SHA-256: `31536b484a3fc9e54fce7d3c570fa41440a1ccb043e4313b13c078061342f542`.
The preserved historical archive remains reproducible with fixed timestamps. Newly generated packages use component-oriented filenames.

Current backup dependency revision 6 pins the Composition and Host component trees to the v1.0.0 engineering baseline `8fa191250ae68274182570c65f067f7a60f85625`. The earlier pins predated the reconciliation regression and Host transport updates already included in that baseline. Component-tree equality, Project Graph equality, source checksum and patch reconstruction checks remain required; later component changes require a new dependency revision.

## Open review gates
Composition revision `7ab43f99f4cc62be2a9b0ce9ce1155283a429768` explicitly approved E0, so the initial review dependency is closed. The recorded production-consumer gate remains partial only because Audio is still contractual: Project Graph/Godot are accepted and Blender head `f492f13` passed run 36942759168 with both Effect Conformance findings resolved. Separately, the recorded repo-wide dependency audit is red because `yoke-derive 0.8.3` was yanked; this subsystem does not alter the global lockfile.
Do not infer main merge, release approval, R16 closure, security certification, global noninterference, crash durability or Broker E2E from the bounded native harness.
