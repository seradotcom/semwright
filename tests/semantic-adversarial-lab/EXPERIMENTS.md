# Frozen experiments and attribution

## Initial suite

Suite `33592bd11573767967f80de11d27012fa5abe689` selected 30 lab-only controls. Run `36502713063`, selection job `109197045588`, was still queued at 2026-09-29T00:34:34Z. No executed-test result may be inferred from that submission. A subsequent revision includes the same controls plus independently authored Composition cases; a queued superseded run may be cancelled only as lab maintenance, retaining this record.

## Composition target

Target `7ed5b848e4d2e7af235d6166e6f93e0cf0bac90d` is the explicit Composition feature snapshot. Seventy cases exercise public types/APIs via a lab-owned example in a separate disposable build copy. The original checkout and product sources remain unmodified. The example's source digest, compile-time target SHA, Cargo.lock digest, compiler version, binary digest and structured compiler artifact are recorded. This is not Broker or native-application acceptance.

The registry encodes independent expectations before execution. Three epoch/permit cases and three evidence/coverage cases are review candidates, not predeclared vulnerabilities. Contract failures require owner/reachability triage; evidence mapping may belong to the Effect Conformance E0 layer rather than the existing Composition aggregate. Neither changing the expected result to observed output nor silently modifying product code is allowed.

Two target evaluator mutations remove required FAIL precedence and exhaustive-observation enforcement. Mutants apply only as recorded diffs in the disposable build copy and retain target SHA + diff digest + suite SHA. An unmutated family must first pass. Build failures are not killed mutants. A mutant is killed only when the independently parsed observation contradicts the registered expectation. Both source restoration and the untouched audited checkout are checked afterward.

The 64 seeded JSON roundtrips are a bounded deterministic corpus, not broad fuzz coverage. Native save/reopen, adversarial graph stores, active native content, complete lifecycle fault injection and the joint candidate remain open. Other subsystem green lanes are not lab evidence.

## Continuation: oracle fixes, audio/AV and evidence ingestion

Run `36502713063` ultimately completed CANCELLED without executing its boundary tests. Run `36503826313` at suite `8ec7a74c3518216feb02ba1b9c55cf3183a0da2a` completed only the selector, then remained queued. The lab requested cancellation at 2026-09-29T01:04Z; both boundary jobs were cancelled without steps. Its empty `always()` selected-gate still held the concurrency slot, so G requested force cancellation after verifying that no boundary process was active. The private cancellation receipt records the actual action; no unrelated run was cancelled.

Suite `85d39e89232c8bbf80e0fce98e097b36e6c05860` added audio/AV and historical oracle controls (189 registered cases). Its run `36505619184` was observed pending and then cancelled after the newer push (the cancelling actor was not independently established); this is not executed evidence.

Suite `fe78c7b048d1e88f6646f28d7ac28d3b9f84c119`, run `36506309475`, includes 217 registered cases and independently validates ingestion of the resulting artifacts. Product targets remain Composition `7ed5b848e4d2e7af235d6166e6f93e0cf0bac90d` and Audio `11b40fb0c59473bc0f007a879bf44c732d22b700`; main was not substituted for either. Actual run receipts, not this submission record, determine whether tests executed.

The resumed administrative main observation was `7a3bae71144bf2c2278b34fc5743e0ceed6dddd1`; it is not the frozen lab baseline, not a tested combined candidate and not a substitute for the Composition/Audio results. Additional source inspection was blocked by the remote tool. No product edit or alternative access bypass followed that block.

## First actual hosted boundary result

Run `36506309475`, suite `fe78c7b048d1e88f6646f28d7ac28d3b9f84c119`, selftest job `109210343273`, failed at enclosure preflight before any of its 82 registered controls executed. Artifact `11007479124` was downloaded and hash-verified; raw receipt SHA-256 is `26aaf1ae12223849b9d2e8dda6277d4aed4b51cf242c374650f8602d597d858c`. The report had no failed-control detail because the wrapper discarded captured stdout on a nonzero probe exit. The underlying enclosure cause was not yet established.

G-LAB-003 is this diagnostic-loss defect in the lab, not a product vulnerability. Diagnostic instrumentation was published in `9f2afd7776149e44884cc2eaa5fd816af57960e1`; its selector still contained four lanes because the edit helper stopped before the selector write. This follow-up applies the selftest-only selector and preserves product-enclosure diagnostics through cleanup. No 9f2afd7 run is claimed to have been selftest-only.

The diagnostic records environment key names, never values. All existing isolation guards remain unchanged. The next selector requests only `selftest`, avoiding Rust/native work while the shared prerequisite is investigated. The other 135 product contract cases remain registered and NOT_RUN. Per-case selftest outcomes now preserve their structured assertion result while the overall gate still requires a consistent process exit and all required cases to pass; one failing case is not mislabeled as every case failing.
