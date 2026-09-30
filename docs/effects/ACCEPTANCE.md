# F acceptance — implementation source 0afad4b4ceac59434ae938d246f96d8d4351498e

Frozen main: b736d41b61c4a4146c9e75c16796e251b025e69f. A C0: 26602e4b25929be869d69ef28fef4dd9713180d7. C P0: 6ee52b428310370d3ad438a13964086a63f48367.
E0: dd6d22d6ec6c7c5ef378da58ed75ca18b25ba5ff. PR: #172. State: NATIVE_COMPLETE_INTEGRATION_REVIEW_PENDING.

| Requirement | State | Exact evidence |
|---|---|---|
| F01 | BLOCKED | EFFECT_GAP_ANALYSIS.md published; A owner approval is still absent |
| F02 | PASS | F depends on A; C is dev-only; no duplicate kernel/vault/policy |
| F03 | PASS | authority/effect bounds and negative grant tests in run 36683875483 |
| F04 | PASS | typed predicates, strict schemas/budgets, 15,000 bounded schema mutations |
| F05 | PASS | owner/base/request/operation/plan/contract/scope replay-substitution tests |
| F06 | PASS | 288-case truth table, vacuity, monotonic properties and killed verdict mutant |
| F07 | PASS | independent observer execution plus native post-write readback-fault => UNKNOWN/no evidence |
| F08 | PASS | paging/count/cursor/race/truncation/final-page tests; count guard mutant killed |
| F09 | PASS | Godot native save/fresh reopen/external-resource mutants in Linux F lane |
| F10 | PASS | Blender native save/reopen/GLB membership plus external/membership mutants |
| F11 | PASS | workflow-scoped nine-dimension Godot/Blender quality reports |
| F12 | PARTIAL | A/C compile consumers; D exact native consumer PASS; E certified consumer exists but F semantic finding remains; B compatibility is contractual only |
| F13 | PASS | bounded scope/attribution tests; no global noninterference/rollback/causality claim |
| F14 | PASS | exact-SHA F CI, 3/3 targeted mutants, reproducible source ZIP, audit/deny, workspace fuzz, packaging lifecycle |
