# Effects and driver conformance — F

## Product contracts
- EFFECT_GAP_ANALYSIS.md maps A-owned semantics to additive F work.
- EFFECT_SEMANTICS.md defines intention, authorization, obligations, evidence and verdicts.
- E0_ADAPTER_CONTRACT.md defines the trusted producer boundary.
- DRIVER_QUALITY_CONTRACT.md defines nine workflow-scoped quality dimensions.
- ACCEPTANCE.md and INTEGRATION.md carry requirement and consumer status.

The implementation is `crates/effect-conformance`. Runtime dependency direction is F -> A semantic-composition. C project-graph is dev-only. F does not own A's reports, PlanVault, canonicalization or policy authority.

## Tested implementation
Implementation source `d2cfd86a2ee064aa5de8f0a8944319edf6dbb060` passed F release run 36942492444 on Ubuntu/Windows/macOS. Linux additionally executed pinned Godot 4.7.2 and Blender 4.5.14 native conformance plus targeted mutations.
The Linux F artifact requested/executed 40/40 tests with zero skips, including the explicit Forbidden-obligation regression, killed all three targeted mutants and generated bounded native receipts/quality reports. Current-SHA Quality 36942497243, Packaging 36942497427 and Supply-chain 36942497317 are SUCCESS. Dependency/coverage/fuzz 36942497359 has coverage/fuzz PASS but its audit job is red because the current RustSec database marks unchanged `yoke-derive 0.8.3` as yanked.

## Reconstructive backup
`backup/semwright-effect-conformance-F-d2cfd86.zip` is the CI-produced source backup for the tested implementation SHA.
SHA-256: `31536b484a3fc9e54fce7d3c570fa41440a1ccb043e4313b13c078061342f542`.
It contains `F_SOURCE.patch`, `SOURCE_MANIFEST.json` and `RESTORE.md`; fixed timestamps make the source package reproducible.

## Open review gates
A final `7ab43f99f4cc62be2a9b0ce9ce1155283a429768` explicitly approved E0, so F01 is closed. F12 remains partial only because B is still contractual: C/D are accepted and E exact head `f492f13` passed run 36942759168 with both F findings resolved. Separately, the current repo-wide dependency audit is red because `yoke-derive 0.8.3` is now yanked; F does not alter that global lockfile.
Do not infer main merge, release approval, R16 closure, security certification, global noninterference, crash durability or Broker E2E from the bounded native harness.
