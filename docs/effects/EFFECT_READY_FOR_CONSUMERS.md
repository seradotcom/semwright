# EFFECT_READY_FOR_CONSUMERS — checkpoint, not final readiness

E0 is published for review at dd6d22d6ec6c7c5ef378da58ed75ca18b25ba5ff in PR #172. E0 source compiled and its three initial tests passed at source 5a4f83d63664d2320a2fc326f65b92dab03fe9a5, but the overall job failed formatting. That is partial test evidence, not a green integration gate.

The expanded evaluator was pushed at 63a8a04c14cc50fe41e7fe3408957e368c47aab1. Further local commits through 19ad98ed13c55000f30718f3837f214941f72e1b consume C P0, bind A plan/budget requirements and recheck channel identity. Those commits are NOT confirmed pushed or tested.

## Adapter entry points
EffectContract -> EvaluationContext -> EvidenceAdapter -> collect -> evaluate -> A VerificationReport plus coverage metadata. collect_untrusted deliberately supplies no trusted producer identity. validate_plan checks the exact A PreparedPlan and effects.contract digest before integration accepts the context.

Required adapter facts: owner/request/operation/plan/contract, resource base, provider session/generation, method/version/source, exact observed addresses, actual values, consistent coverage, attribution, artifact identity and complete bounded enumeration where needed. Observers use existing Broker authorization; none of these data structures grants permission.

C P0 consumer source is tests/project_consumer.rs; it preserves FAIL/UNKNOWN through ExecutionReceipt validation/admission/serialization without setting cache_safe. Its current-SHA execution is pending.
D/E own real provider-native extraction/admission. F's native example is a disposable product-adapter conformance harness, not a production Broker integration. Figma/Motion/Audio compatibility tests are explicitly contractual.

READY_FOR_INTEGRATION=false. NATIVE_EVIDENCE_CONFIRMED=false. OWNER_A_APPROVAL=pending. See ACCEPTANCE.md and INTEGRATION.md for exact runs, source pins and remaining obligations. Do not inherit a native claim from any other app or prior source SHA.
