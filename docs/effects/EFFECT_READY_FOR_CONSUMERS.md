# EFFECT_READY_FOR_CONSUMERS

E0 is published at `dd6d22d6ec6c7c5ef378da58ed75ca18b25ba5ff`. The current evaluator implementation tested at `0afad4b4ceac59434ae938d246f96d8d4351498e` has exact-SHA portable and native evidence.

## Stable adapter boundary
`EffectContract -> EvaluationContext::validate_plan -> EvidenceAdapter -> collect -> evaluate -> A VerificationReport + coverage`.
Adapters obtain owner/provider/session/generation from authenticated execution state. Client JSON cannot mint trusted EvidenceBatch, PASS, exhaustive coverage or authorization.
Required facts remain owner/request/operation/plan/contract, current bases, method/version/source, observed scope, values, consistency, attribution, artifact binding and bounded enumeration.

## Evidence status
F release run 36683875483: SUCCESS on Ubuntu 24.04, Windows 2025 and macOS 15.
Ubuntu native: Godot baseline/external-mutant/observation-mutant/readback-fault and Blender baseline/external-mutant/membership-mutant/readback-fault all matched expected PASS/FAIL/UNKNOWN.
Readback faults leave all required checks UNKNOWN with empty evidence. Recovery/crash durability remains UNKNOWN/NOT_TESTED.
D exact source 655a233 is a production consumer with run 36742703497 SUCCESS. E has a certified production consumer but still carries the preservation obligation/scope finding documented in INTEGRATION.md.
C P0 and A PreparedPlan compile consumers are covered. Figma/Motion/Audio is a contractual compatibility consumer, not native evidence for those apps.

EFFECT_READY_FOR_CONSUMERS=true.
EFFECT_READY_FOR_INTEGRATION=false.
NATIVE_EVIDENCE_CONFIRMED=true for the bounded F Godot/Blender workflows only.
OWNER_A_APPROVAL=pending.
R16_CLOSED=false.
