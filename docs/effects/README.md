# Effects and driver conformance — F

## Product contracts
- EFFECT_GAP_ANALYSIS.md maps existing A semantics to additive F work.
- EFFECT_SEMANTICS.md specifies intention, authorization, predicates, evidence and verdicts.
- E0_ADAPTER_CONTRACT.md explains the trusted producer boundary.
- DRIVER_QUALITY_CONTRACT.md defines nine workflow-scoped evidence dimensions.
- SECURITY_DELTA.md and RELEASE_IMPACT.md describe limits and open integration gates.

The implementation is crates/effect-conformance. It consumes A reports/PlanVault contracts and existing semwright-types command descriptors. The only C dependency is the P0 dev consumer. No A/C source is modified. Native D/E production extractors remain owned by their authors.

## Current evidence discipline
The named checkpoint documents preserve their historical SHAs; do not treat their status paragraphs as a live dashboard. Current evidence is the exact-SHA Actions artifact, the PR #172 handoff and the private coordination F.json. No file named READY is itself an acceptance receipt.

At source 42204ac6a6f3c66ba66de5adfe689d8633bb7c74, 34 contract tests executed with zero skips. The overall job failed strict test naming; identifiers were corrected in a00bc6a8cf422cb002f55a4b5cd15f406cd234e0 without relaxing assertions. Source added later is not retrospectively tested by that run. Native and portable acceptance, targeted mutation execution and final consumer integration remain unconfirmed at this handoff.

## Reconstructible backup
backup/semwright-effect-conformance-F-source.zip snapshots source a00bc6a8cf422cb002f55a4b5cd15f406cd234e0. It contains a complete F source patch against C P0 6ee52b428310370d3ad438a13964086a63f48367, exact file hashes and a restore runbook. Its SHA-256 is a329e77f1ead34e5243c3cf32b5dc31c5eefc47bdd94f490f31a7d7c885a20f4.
The patch restores all 40 source files and Cargo.lock byte-for-byte. Two independent packaging runs produced identical ZIP bytes. Neither operation runs Cargo, tests or native applications.
The diagnostic lane also generates a source backup for its own exact SHA using scripts/effects/package_source.py; it does not recursively bundle older backups.

## Open gates
Owner A review; D/E production adapter receipts; final A/B/C integration; native Godot/Blender negatives; targeted mutations; Linux/Windows/macOS portable gates; dependency/license audit and clean installation. Do not merge main, publish a release, claim security certification or close R16 from this candidate.
