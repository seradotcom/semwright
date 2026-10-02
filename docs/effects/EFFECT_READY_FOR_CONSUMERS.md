# EFFECT_READY_FOR_CONSUMERS

E0 is published at `dd6d22d6ec6c7c5ef378da58ed75ca18b25ba5ff`. The current evaluator implementation tested at `d2cfd86a2ee064aa5de8f0a8944319edf6dbb060` has exact-SHA portable and native evidence.

## Stable adapter boundary
`EffectContract -> EvaluationContext::validate_plan -> EvidenceAdapter -> collect -> evaluate -> A VerificationReport + coverage`.
Adapters obtain owner/provider/session/generation from authenticated execution state. Client JSON cannot mint trusted EvidenceBatch, PASS, exhaustive coverage or authorization.
Required facts remain owner/request/operation/plan/contract, current bases, method/version/source, observed scope, values, consistency, attribution, artifact binding and bounded enumeration.

## Evidence status
F release run 36942492444: SUCCESS on Ubuntu 24.04, Windows 2025 and macOS 15; 40/40 Linux contract tests executed with zero skips, including the explicit Forbidden-obligation regression.
Ubuntu native: Godot baseline/external-mutant/observation-mutant/readback-fault and Blender baseline/external-mutant/membership-mutant/readback-fault all matched expected PASS/FAIL/UNKNOWN.
Readback faults leave all required checks UNKNOWN with empty evidence. Recovery/crash durability remains UNKNOWN/NOT_TESTED.
D exact source 655a233 is a production consumer with run 36742703497 SUCCESS. E exact head `f492f13a028f781d9ca55631764578f5b327eb1b` contains both requested F semantic fixes (Forbidden unmanaged preservation and explicit unmanaged-scene/source-projection scope), and authoring run 36942759168 is SUCCESS.
C P0 and A PreparedPlan compile consumers are covered. Figma/Motion/Audio is a contractual compatibility consumer, not native evidence for those apps.

EFFECT_READY_FOR_CONSUMERS=true.
EFFECT_READY_FOR_INTEGRATION=false.
NATIVE_EVIDENCE_CONFIRMED=true for the bounded F Godot/Blender workflows only.
OWNER_A_APPROVAL=approved at A final `7ab43f99f4cc62be2a9b0ce9ce1155283a429768` / PR #172 issuecomment-5942070000.
R16_CLOSED=false.
