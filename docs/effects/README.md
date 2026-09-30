# Effects and driver conformance — F

## Product contracts
- EFFECT_GAP_ANALYSIS.md maps A-owned semantics to additive F work.
- EFFECT_SEMANTICS.md defines intention, authorization, obligations, evidence and verdicts.
- E0_ADAPTER_CONTRACT.md defines the trusted producer boundary.
- DRIVER_QUALITY_CONTRACT.md defines nine workflow-scoped quality dimensions.
- ACCEPTANCE.md and INTEGRATION.md carry requirement and consumer status.

The implementation is `crates/effect-conformance`. Runtime dependency direction is F -> A semantic-composition. C project-graph is dev-only. F does not own A's reports, PlanVault, canonicalization or policy authority.

## Tested implementation
Implementation source `0afad4b4ceac59434ae938d246f96d8d4351498e` passed F release run 36683875483 on Ubuntu/Windows/macOS. Linux additionally executed pinned Godot 4.7.2 and Blender 4.5.14 native conformance plus targeted mutations.
The same SHA passed Quality 36683883701, dependency/coverage/fuzz 36683883572, packaging lifecycle 36683883626 and supply-chain 36683883597.
The Linux F artifact requested/executed 39/39 tests with zero skips, killed all three targeted mutants and generated bounded native receipts/quality reports.

## Reconstructive backup
`backup/semwright-effect-conformance-F-0afad4b.zip` is the CI-produced source backup for the tested implementation SHA.
SHA-256: `81dce5784b01693f1c221e65937c25d4640ecea1a4c74aa384451325bf591b8f`.
It contains `F_SOURCE.patch`, `SOURCE_MANIFEST.json` and `RESTORE.md`; fixed timestamps make the source package reproducible.

## Open review gates
A owner approval remains missing. F12 remains partial: D is exact-certified, E consumes F but has an unresolved preservation obligation/scope finding, and B remains a contractual compatibility consumer in this frozen graph.
Do not infer main merge, release approval, R16 closure, security certification, global noninterference, crash durability or Broker E2E from the bounded native harness.
