# Effect gap analysis — E0

Baseline: `b736d41b61c4a4146c9e75c16796e251b025e69f`.
Composition observed: `7ed5b848e4d2e7af235d6166e6f93e0cf0bac90d` (PR 168).
Consumed shared contract: `26602e4b25929be869d69ef28fef4dd9713180d7`.
C1 `ba161085ff6df5d791b2b643a3d4ea708c9d495e` was read, including AUDIO_AV_CONTRACT;
`crates/semantic-composition` has no diff from C0 to the observed Composition tip.
Only isolated C0 history is merged, not the Composition media/native implementations.

| Existing Composition surface | Remaining semantics | Effect Conformance additive implementation/test |
|---|---|---|
| EffectClass, TypedOperation reads/writes, ChangeSet | descriptor classes are not per-operation bounds or grants | typed limits intersect implementation/Broker; negative grants |
| ValidationReport::verdict | already preserves required FAIL ahead of UNKNOWN; rejects duplicate/version-zero/vacuous | reuse unchanged; truth table and explicit vacuity metadata |
| VerificationReport::verdict | completed distinct from verified already | reuse unchanged, normalize insufficient evidence before aggregation |
| ObservationRef base/source/scope/exhaustive | no request/owner/plan binding or authenticated producer linkage; empty scope can PASS | private collected batch; transport/identity/scope/method guards |
| required_rules pinned by ProfileDescriptor | no predicate/version/units coverage pin | canonical effect-contract digest pinned by trusted execution |
| PlanVault/controller | authority and budget lifecycle already present | no new vault/controller; adapters authorize observations through existing route |
| BaseStateSet/check_fresh | real per-resource state, no global revision | exact post-state binding; causal ambiguity separate from differences |
| Godot scene_save #171 | in-process reload and external material sentinel | fresh-process readback + external animation/material mutants |
| GLB adapter on main; #154 still OPEN at 74671c11dda2133ce6af939896c49cdbb6ba47d5 | named collection closure is not decoded membership proof | native export + decoded member oracle + excluded-object mutant |

Dependency direction: drivers/harness -> effect-conformance -> semantic-composition.
This work is scoped to effect-conformance, `docs/effects`, `scripts/effects` and its diagnostic lane.
No Composition source is edited. Shared changes: one lockfile package entry only, in a separate commit.
Composition review is REQUESTED, not inferred from silence. The initial review dependency remains open at this checkpoint.
Potential Composition documentation proposal: source-level documentation that bare reports are data, not authenticity;
F can provide normalized RuleResults without modifying the current wire schema.

Initial main checks: 35 success, 1 failure (atspi-backend) at inspection, not all-green.
That unrelated failure is retained as a baseline limitation, not fixed or hidden by this work.
Native tests from #171/#154 are prior evidence, NOT evidence for the new evaluator.
