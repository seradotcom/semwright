# F integration handoff

Implementation source: `0afad4b4ceac59434ae938d246f96d8d4351498e`.
Frozen main: `b736d41b61c4a4146c9e75c16796e251b025e69f`.
A C0: `26602e4b25929be869d69ef28fef4dd9713180d7`; C P0: `6ee52b428310370d3ad438a13964086a63f48367`; E0: `dd6d22d6ec6c7c5ef378da58ed75ca18b25ba5ff`.

## Proven integration
A remains owner of Composition reports, canonicalization, PlanVault, policy-facing plan state and shared types. F consumes those APIs and never grants authority.
C's P0 receipt consumer preserves required FAIL/UNKNOWN without promoting cache/safety.
D source `655a233b0000a21702dfe3343086aeff14c3f26c` routes authenticated native state through `EvaluationContext::validate_plan -> EvidenceAdapter -> collect -> evaluate`; exact certification run 36742703497 passed native, persistence, export, hostile, cross-app and package gates.
E certified source `3d04d8465dcfa94d6cbf548fcaf343f69ac5838f` uses `EvidenceAdapter -> collect -> evaluate` and its native suite is green. F still requires its unmanaged-preservation rule to be normative Forbidden and its whole-scene observation to bind an explicit whole-scene/unmanaged projection scope.
Figma/Motion/Audio compatibility is contractual in F. The frozen F branch contains no B audio crate; it does not claim B production integration.

## Exact evidence
F release run 36683875483 is SUCCESS at 0afad4b on Linux/Windows/macOS. Linux executed both native backends and targeted mutation tests.
Quality run 36683883701, dependency/coverage/fuzz run 36683883572, packaging run 36683883626 and supply-chain run 36683883597 are SUCCESS at the same source SHA.
The Linux F artifact records 39 requested/39 executed tests, zero skipped, native Godot/Blender receipts, quality matrices, 3/3 killed targeted mutants and the reconstructive source ZIP.

## Remaining integration gates
A owner approval is still required by F01.
B production integration remains outside this frozen dependency graph; retain the contractual compatibility consumer until A/B integrated media exposes a stable boundary.
E must resolve the two concrete semantic findings above before F treats its preservation receipt as conformant.
I/global integration may consume this F implementation and exact evidence, but must not reinterpret partial F12 or missing A approval as PASS.
No merge to main, release, R16 closure, security certification, global noninterference or cross-app rollback is implied.
