# Effect Conformance integration

Implementation source: `d2cfd86a2ee064aa5de8f0a8944319edf6dbb060`.
Frozen main: `b736d41b61c4a4146c9e75c16796e251b025e69f`.
Composition C0: `26602e4b25929be869d69ef28fef4dd9713180d7`; Project Graph P0: `6ee52b428310370d3ad438a13964086a63f48367`; E0: `dd6d22d6ec6c7c5ef378da58ed75ca18b25ba5ff`.

## Proven integration
Composition remains authoritative for reports, canonicalization, PlanVault, policy-facing plan state and shared types. Effect Conformance consumes those APIs and never grants authority. Composition revision `7ab43f99f4cc62be2a9b0ce9ce1155283a429768` explicitly reviewed E0 `dd6d22d6ec6c7c5ef378da58ed75ca18b25ba5ff`, closing the recorded review dependency.
The Project Graph P0 receipt consumer preserves required FAIL/UNKNOWN without promoting cache/safety.
Godot source `655a233b0000a21702dfe3343086aeff14c3f26c` routes authenticated native state through `EvaluationContext::validate_plan -> EvidenceAdapter -> collect -> evaluate`; exact certification run 36742703497 passed native, persistence, export, hostile, cross-app and package gates.
Blender exact head `f492f13a028f781d9ca55631764578f5b327eb1b` uses `EvidenceAdapter -> collect -> evaluate`, includes `Obligation::Forbidden` for unmanaged preservation and binds an explicit `unmanaged-scene/source-projection` address. Dedicated exact-head authoring run 36942759168 is SUCCESS, so the Blender portion of the recorded consumer gate is closed.
Figma/Motion/Audio compatibility is contractual in Effect Conformance. The frozen conformance branch contains no production audio crate; it does not claim production audio integration.

## Exact evidence
Effect Conformance release run 36942492444 is SUCCESS at `d2cfd86a2ee064aa5de8f0a8944319edf6dbb060` on Linux/Windows/macOS. Linux executed both native backends, the explicit Forbidden-obligation regression and targeted mutation tests.
Current packaging run 36942497427 and Quality run 36942497243 are SUCCESS at d2cfd86. Dependency/coverage/fuzz 36942497359 has coverage and fuzz PASS, while dependency audit FAILS only because the current RustSec database marks unchanged `yoke-derive 0.8.3` as yanked. Supply-chain 36942497317 is SUCCESS across both release-bundle jobs and pinned Nix; attestation was skipped/not claimed. The prior production-equivalent source 0afad4 passed the same lock through audit/deny; d2cfd86 changes only the Forbidden-obligation regression test.
The Linux Effect Conformance artifact records 40 requested/40 executed tests, zero skipped, native Godot/Blender receipts, quality matrices, 3/3 killed targeted mutants and the reconstructive source ZIP.

## Remaining integration gates
The initial review dependency is closed by Composition review.
Production audio integration remains outside this frozen dependency graph; retain the contractual compatibility consumer until the audio subsystem publishes a production Effect Conformance consumer on a recertified audio SHA.
The Blender production consumer receipt is accepted at f492f13 / run 36942759168. The recorded consumer gate then waits only for the audio subsystem to publish and certify its production Effect Conformance consumer.
Global integration may consume this Effect Conformance implementation and exact evidence, but must not reinterpret a partial consumer gate as PASS.
No merge to main, release, R16 closure, security certification, global noninterference or cross-app rollback is implied.
