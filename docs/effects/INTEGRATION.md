# F integration order and checkpoint

Frozen main: b736d41b61c4a4146c9e75c16796e251b025e69f.
A C0: 26602e4b25929be869d69ef28fef4dd9713180d7, consumed through normal merge.
A C1 was inspected at 7ed5b848e4d2e7af235d6166e6f93e0cf0bac90d; the shared semantic-composition crate is byte-identical to C0.
C P0: 6ee52b428310370d3ad438a13964086a63f48367, consumed through normal merge for a dev-only receipt consumer.

Published E0: dd6d22d6ec6c7c5ef378da58ed75ca18b25ba5ff.
Last confirmed pushed source: 63a8a04c14cc50fe41e7fe3408957e368c47aab1, PR #172.
Local committed source: 19ad98ed13c55000f30718f3837f214941f72e1b. The subsequent checkpoint documentation is uncommitted.
Do not assume local commits are already on origin. Do not create another branch/PR or reset this worktree to the older pushed source.

## Consumer sequence
A remains owner of the kernel, reports, vault, canonicalization, profiles and budgets. Review EFFECT_GAP_ANALYSIS.md; the approval request is on PR #168. There is no approval inferred from silence.
C stores A VerificationReport plus determinant/coverage data; the P0 consumer preserves required FAIL and UNKNOWN. Receipt admission does not mean safe.
D/E keep their native extractors in their own source. F provides EvidenceAdapter, collect/evaluate, EnumerationPage audits and normalized A reports. No driver worktree is edited by F.
A/B consumers retain their own typed media measurement semantics and units; the current Figma/Motion/Audio tests are contractual, not native acceptance.

## CI continuation
First verify the existing queued native run 36502813566 / job 109197365116 against source 63a8a04. Its result was not retrievable after the tool safety block.
Commit/review the current local delta, push a NEW SHA, then inspect its NEW push-triggered effect-conformance run. A rerun of an old run does not test these newer commits.
Use scripts/effects/lane.json with the registered enum: effect-contract, godot, blender, native-all, mutations or release. No shell command is accepted as a selector.
Native examples prove product-adapter conformance only. They are not a substitute for production Broker/driver authoring integration receipts from D/E.

Finish ACCEPTANCE.md row-by-row, current-SHA gates, owner review, targeted mutants, portable checks, clean packaging and evidence manifests. Do not merge main, publish a release, make a demo, mark R16 closed or issue trust badges from this checkpoint.
