# G Composition adversarial findings

Exact attacked A target: 7ed5b848e4d2e7af235d6166e6f93e0cf0bac90d
G suite: 891ea3f024398818407b8477be882c7943dd0c3a
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

Three verification failures (G-VERIFY-009..011) were G oracle defects: A is the generic report aggregator, while F's trusted effect evaluator owns method/version/source/scope/observability binding before producing A's report. They are not product findings.

## G-FIND-A-001 — stale BeginPermit can complete a different attempt incarnation

Cases: G-PLAN-022, G-PLAN-023, G-PLAN-024.

BeginPermit carries owner, root, index, digest. PlanVault::finish() resolves those values against the current vault/root and accepts the permit whenever the current attempt at that index has the same digest and is Applying. The permit carries no vault identity, root incarnation/epoch, or generation nonce.

Independent reproductions:

1. Revoke owner, reissue the same root ID, begin a new attempt, then finish with the old pre-revoke permit. The old permit is accepted and completes the new attempt.
2. Create a second independent PlanVault with the same owner/root/index/digest and finish its current attempt using a permit minted by the first vault. The foreign-vault permit is accepted.
3. Let the original root expire/reap, reissue the same root ID, begin a new attempt, then finish with the expired-root permit. The expired permit is accepted.

This conflicts with A's handoff statement that restart/expiry/revocation invalidates pending plans. It also allows stale completion authority to cross a root incarnation while the API comment says the private permit prevents replay.

Current PR #168 head ffe7e59c5ba0d28ac9193ec203307dd7969fee4f was inspected after the finding: BeginPermit and finish() retain the same identity fields/check, so this finding is not already fixed by that head.

G will not patch A. Owner A should bind permits to a non-reusable vault/root incarnation (or equivalent unforgeable generation authority), add revoke/expiry/cross-vault completion regressions, publish an explicit FIX_SHA, and hand it to G for exact-SHA retest.
