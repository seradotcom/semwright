# F integration handoff

Implementation source: `d2cfd86a2ee064aa5de8f0a8944319edf6dbb060`.
Frozen main: `b736d41b61c4a4146c9e75c16796e251b025e69f`.
A C0: `26602e4b25929be869d69ef28fef4dd9713180d7`; C P0: `6ee52b428310370d3ad438a13964086a63f48367`; E0: `dd6d22d6ec6c7c5ef378da58ed75ca18b25ba5ff`.

## Proven integration
A remains owner of Composition reports, canonicalization, PlanVault, policy-facing plan state and shared types. F consumes those APIs and never grants authority. A final `7ab43f99f4cc62be2a9b0ce9ce1155283a429768` explicitly reviewed and approved E0 `dd6d22d6ec6c7c5ef378da58ed75ca18b25ba5ff`, closing F01.
C's P0 receipt consumer preserves required FAIL/UNKNOWN without promoting cache/safety.
D source `655a233b0000a21702dfe3343086aeff14c3f26c` routes authenticated native state through `EvaluationContext::validate_plan -> EvidenceAdapter -> collect -> evaluate`; exact certification run 36742703497 passed native, persistence, export, hostile, cross-app and package gates.
E exact head `f492f13a028f781d9ca55631764578f5b327eb1b` uses `EvidenceAdapter -> collect -> evaluate`, includes `Obligation::Forbidden` for unmanaged preservation and binds an explicit `unmanaged-scene/source-projection` address. Dedicated exact-head authoring run 36942759168 is SUCCESS, so the E portion of F12 is closed.
Figma/Motion/Audio compatibility is contractual in F. The frozen F branch contains no B audio crate; it does not claim B production integration.

## Exact evidence
F release run 36942492444 is SUCCESS at `d2cfd86a2ee064aa5de8f0a8944319edf6dbb060` on Linux/Windows/macOS. Linux executed both native backends, the explicit Forbidden-obligation regression and targeted mutation tests.
Current packaging run 36942497427 and Quality run 36942497243 are SUCCESS at d2cfd86. Dependency/coverage/fuzz 36942497359 has coverage and fuzz PASS, while dependency audit FAILS only because the current RustSec database marks unchanged `yoke-derive 0.8.3` as yanked. Supply-chain 36942497317 is SUCCESS across both release-bundle jobs and pinned Nix; attestation was skipped/not claimed. The prior production-equivalent source 0afad4 passed the same lock through audit/deny; d2cfd86 changes only the Forbidden-obligation regression test.
The Linux F artifact records 40 requested/40 executed tests, zero skipped, native Godot/Blender receipts, quality matrices, 3/3 killed targeted mutants and the reconstructive source ZIP.

## Remaining integration gates
F01 is closed by A owner approval.
B production integration remains outside this frozen dependency graph; retain the contractual compatibility consumer until B publishes a production F consumer on a recertified audio SHA.
E's production consumer receipt is accepted at f492f13 / run 36942759168. F12 now waits only for B to publish and certify its production Audio consumer.
I/global integration may consume this F implementation and exact evidence, but must not reinterpret partial F12 as PASS.
No merge to main, release, R16 closure, security certification, global noninterference or cross-app rollback is implied.
