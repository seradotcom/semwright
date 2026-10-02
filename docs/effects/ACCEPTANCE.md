# F acceptance checkpoint at 19ad98e — historical, NOT READY FOR INTEGRATION

Frozen main: b736d41b61c4a4146c9e75c16796e251b025e69f. A C0: 26602e4b25929be869d69ef28fef4dd9713180d7. C P0: 6ee52b428310370d3ad438a13964086a63f48367.
Last confirmed pushed SHA: 63a8a04c14cc50fe41e7fe3408957e368c47aab1. Local committed SHA: 19ad98ed13c55000f30718f3837f214941f72e1b; subsequent checkpoint documentation is uncommitted.

| Requirement | Implementation / test | Evidence and remaining obligation |
|---|---|---|
| F01 | EFFECT_GAP_ANALYSIS.md; no A source edits | Analysis published; owner A approval requested, not received |
| F02 | effect-conformance -> semantic-composition; C dev consumer | E0 compiles at 5a4f83d; current integrated SHA not tested |
| F03 | check_effect_bounds; client_allow_does_not_grant_authority | E0 negative grant test passed; production Broker adapter integration open |
| F04 | predicate.rs, strict_decode, schemas | E0 tolerance/units/schema tests passed; new fuzz suite not confirmed |
| F05 | evidence.rs/evaluator.rs; replay/substitution tests | Contract test phase passed at 37cfb4f; latest plan/channel additions untested |
| F06 | A report aggregation; 288-case truth table | Contract test phase passed at 37cfb4f; complete current-SHA gate still open |
| F07 | native product ReadbackOps and independent readback guard | Native oracle scripts implemented; no native success confirmed |
| F08 | enumeration.rs; cursor/count/race/truncation tests | Contract test phase passed at 37cfb4f; driver-specific paging adapters open |
| F09 | Godot save/readback/fresh-process probes; material/animation sentinels | Native lane submitted at 63a8a04; final result not retrieved |
| F10 | Blender Commands/export/reopen + GLB node decoder and mutant | Native lane submitted at 63a8a04; final result not retrieved |
| F11 | nine quality dimensions; workflow_quality | Implementation and negative examples present; generated native matrix pending |
| F12 | A types; C P0 receipt consumer; Figma/Motion/Audio contractual consumers | C test dependency integrated locally; D/E/A/B production receipts pending |
| F13 | attribution/coverage guards; explicit bounded root inventories | No global noninterference, transaction, crash durability or security claim |
| F14 | selective workflow, bounded mutation fuzz, targeted mutant runner | Fuzz/mutation/portable matrix/audit/clean installation/final packaging gates remain open |

## Exact CI observations
Run 36501485846, job 109193111624, source 5a4f83d63664d2320a2fc326f65b92dab03fe9a5: 3 E0 tests executed, none skipped; clippy/rustdoc passed; overall job FAILED on formatting. Its patch was applied subsequently.
Run 36502247967, job 109195550126, source 37cfb4fbd1216e3ef336f26bd192cd599e5d97f4: test/receipt phase passed; overall job FAILED on clippy::collapsible_if in enumeration. Source fix was committed in 63a8a04.
Run 36502813566, job 109197365116, source 63a8a04c14cc50fe41e7fe3408957e368c47aab1: last successfully observed QUEUED. Later run-status commands were blocked by the tool safety service; no conclusion or native acceptance is inferred.
