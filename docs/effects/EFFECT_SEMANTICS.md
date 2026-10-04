# Effect semantics and evidence — E0 / evaluator candidate

Intention, authorization, allowed effects, required effects, forbidden conditions, observation and verification are separate. A client allowed-effect list is never a policy grant. `check_effect_bounds` intersects declared limits with implementation and Broker bounds supplied by trusted integration; it returns no execution permit.

Composition defines EffectClass, ChangeSet, BaseStateSet, ObservationRef, RuleResult, Verdict, ExecutionStatus, VerificationReport, PlanVault and budgets. Effect Conformance adds closed predicates and evidence requirements. Dependency direction is consumer → effect-conformance → Composition; Project Graph is a test-only dependency of Effect Conformance.

`Required` and `Forbidden` are hard obligations. A forbidden rule expresses the condition that MUST hold, such as `Preserved` or `Absent`; it is not automatically inverted. Preferences remain visible but cannot erase a required failure. Predicates have no eval, script, arbitrary filesystem read or unchecked JSON pointer. Units, epsilon, rule/method versions and the complete contract digest are pinned before execution.

## Verdict table
| Required evidence | Execution | Result |
|---|---|---|
| At least one sufficiently observed false predicate | Any status | FAIL |
| No false predicate, but UNKNOWN/ERROR/UNSUPPORTED/ack/echo/missing evidence | Any status | UNKNOWN |
| All required predicates true with sufficient evidence | completed | PASS |
| All required predicates true | Any other execution status | UNKNOWN |
| No required predicates | Any | Explicit vacuous=true, UNKNOWN; bare A validation is invalid |
| Duplicate rule, version zero or changed pinned contract | Any | Validation error, never PASS |

The executable truth table covers 8 execution statuses and 6 states for each of two required checks (288 combinations). A performs aggregation; F normalizes unsupported or insufficient observations to UNKNOWN first.

## Evidence boundary
`EvaluationContext` and `EvidenceBatch` cannot be deserialized from client JSON. Only trusted compiled adapters collect observations. Imported receipts have no trusted channel identity. Binding covers owner, request, operation, plan/contract digests, resource base, provider session/generation, method/version, source, scope and optional artifact. Channel identity is rechecked after observation. `validate_plan` additionally matches A's PreparedPlan, required rules, scope and unchanged budget; it is not a replacement for PlanVault admission.

A normalized observation covers only the evaluated address. Global absence, exact membership and cardinality require a declared universe, bounded complete pages, stable ordering, consistent snapshot/query/principal binding, unique continuations and a final page. Best-effort/racing/truncated enumeration stays UNKNOWN. Effect Conformance audits provider cursors; it does not issue or authenticate them from raw JSON.

Before/after equality does not establish causality. Concurrent/ambiguous attribution is retained, and a causality-required rule cannot pass on it. Post-hoc detection is not prevention; Host/OS enforcement remains separate. On a violation, callers stop pending operations and retain prior effects, without automatic destructive repair.
