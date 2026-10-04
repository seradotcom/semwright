# Composition adversarial findings

Exact attacked Composition target: 7ed5b848e4d2e7af235d6166e6f93e0cf0bac90d
Lab suite: 891ea3f024398818407b8477be882c7943dd0c3a
Run: 36884906772
Composition job: 110445685891

## Result before oracle triage

- Requested: 70
- Executed: 70
- PASS: 64
- FAIL: 6
- BLOCKED: 0
- NOT_RUN: 0
- Receipt SHA-256: 825e08af7bfd18918399596f1e63a39281d4219d38d8902444fd43fc405ac4d4
- Artifact SHA-256: 4cbeab717cb648e850d9f8fa9740cc596f351ab563416c8f6b6416cfd2e5dc09
- Same-suite selftest: 103/103 PASS.
- G-MUT-001 and G-MUT-002: KILLED.

Three verification failures (G-VERIFY-009..011) were lab oracle defects: Composition is the generic report aggregator, while the trusted Effect Conformance evaluator owns method/version/source/scope/observability binding before producing the report. They are not product findings.

## G-FIND-A-001 — stale BeginPermit can complete a different attempt incarnation

Cases: G-PLAN-022, G-PLAN-023, G-PLAN-024.

BeginPermit carries owner, root, index, digest. PlanVault::finish() resolves those values against the current vault/root and accepts the permit whenever the current attempt at that index has the same digest and is Applying. The permit carries no vault identity, root incarnation/epoch, or generation nonce.

Independent reproductions:

1. Revoke owner, reissue the same root ID, begin a new attempt, then finish with the old pre-revoke permit. The old permit is accepted and completes the new attempt.
2. Create a second independent PlanVault with the same owner/root/index/digest and finish its current attempt using a permit minted by the first vault. The foreign-vault permit is accepted.
3. Let the original root expire/reap, reissue the same root ID, begin a new attempt, then finish with the expired-root permit. The expired permit is accepted.

This conflicts with the Composition lifecycle contract that restart/expiry/revocation invalidates pending plans. It also allows stale completion authority to cross a root incarnation while the API comment says the private permit prevents replay.

Composition PR #168 head 3223bdf0367b4c9de73ef867a560b200f1ab96e1 was inspected after the finding. BeginPermit and finish() still retain the same owner/root/index/digest authority without a vault/root incarnation binding. The newer vault changes cover expiry arithmetic, state guards and sibling Unknown reconciliation, but do not fix this finding.

The lab does not patch the Composition implementation. The fix must bind permits to a non-reusable vault/root incarnation (or equivalent unforgeable generation authority), add revoke/expiry/cross-vault completion regressions, publish an explicit FIX_SHA and undergo an exact-SHA lab retest.

## Closure retest

Composition published FIX_SHA 7ab43f99f4cc62be2a9b0ce9ce1155283a429768. The fix adds private vault and root incarnation identities to BeginPermit and verifies them in PlanVault::finish. The lab retested the unchanged 70-case Composition family on suite bf5a70f2e0f0ec6894b8f43e1a8322f9e1ed9e9f, run 36938854785, job 110625787176. Result: 70/70 PASS, 0 FAIL/BLOCKED/NOT_RUN. G-PLAN-022, G-PLAN-023 and G-PLAN-024 all PASS. Receipt SHA-256: 8623175f1bcc1e4a7f9764698cf2ffa6d5b41fd7e90a932127cb719c07597884. Artifact SHA-256: 9cdd503ce6e5de968577e9e0ce26d08e2aa9edb13e1bcd0acae2bd4201bcc3aa. Status: CLOSED_RETEST_PASS.
