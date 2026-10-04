# Adversarial lab runbook

## Read-only workstation operations

Run only Git/gh, bounded evidence collection, source hashes, JSON/syntax checks and packaging on the workstation. Do not run Cargo, native applications, the selftest, fuzzing or guard mutants locally, even by setting fake CI environment flags. Attacks execute inside the disposable Actions enclosure.

From a clean lab checkout, list the versioned lane matrix with `python3 tests/semantic-adversarial-lab/runner.py matrix`. Commit and push test changes to the dedicated lab branch to create a NEW suite SHA. Query its workflow using `gh run list --repo seradotcom/semwright --workflow semantic-adversarial-lab.yml --branch test/semantic-adversarial-lab`. The final evidence manifest names the exact relevant run rather than assuming the newest branch result.

Collect one explicit experiment (replace the two identifiers together with an observed pair):

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tests/semantic-adversarial-lab/collect_evidence.py \
  --run-id 36506309475 \
  --suite-sha fe78c7b048d1e88f6646f28d7ac28d3b9f84c119 \
  --output <evidence-dir>/36506309475
```

The collector reads source manifests only from the suite commit, validates exact run/job/attempt metadata, refuses oversized artifacts, verifies artifact SHA-256, and independently recomputes case counts/verdicts. It writes raw receipts plus `EXPERIMENT_INDEX.json`, `JOB_PROVENANCE.json`, `EVIDENCE.md` and private untriaged finding records. A successful collector invocation means collection completed, not that the experiment passed.

## Failure triage

Use the actual job database ID from the index with `gh run view RUN_ID --repo seradotcom/semwright --job JOB_ID --log-failed`. Missing artifacts remain BLOCKED. A compile/setup failure is not a product vulnerability; a failed contract observation is not automatically exploitable. Check the healthy control and source contract, preserve expected/actual, and route the minimal report to the relevant subsystem. Fix only lab test defects in the lab tree.

An unchanged-code retry uses the same run and SHA. A product fix or changed harness requires a new commit and new run. Re-execute the before reproducer and every affected family with the same oracle content on FIX_SHA (record both suite commits); keep prior failed receipts. Do not rely on a green author test or a changed expectation.

## Cancellation and storage

Only obsolete lab runs may be cancelled as part of lab maintenance. Preserve their IDs, source/suite SHAs, actual last job state, replacement run and reason. If an ordinary cancellation leaves only an empty obsolete `always()` gate queued, the documented GitHub force-cancel API may terminate that lab run; never use it on unrelated work or to hide a failure. Cancellation is not PASS.

Preserve small source packages, evidence manifests and diagnostic tails. Never download target directories, caches, large binaries/renders or all workflow logs merely to read a verdict. Artifacts expire according to repository retention; keep required sanitized delivery receipts before expiration. Do not delete unrelated caches or source checkouts.

## Reproducible source backup

Use `package_backup.py --commit FULL_DELIVERY_SHA --evidence-dir PATH --output NEW_ZIP_PATH` for bounded packaging. It reads only lab-owned tracked paths from the exact commit, rejects product changes and symlinks, includes a full-index patch against the frozen baseline, and embeds only explicitly selected evidence summaries. ZIP timestamps/order are deterministic; all payloads and the archive receive SHA-256 checks. Packaging performs no attacks or builds.

The delivery commit and each tested suite are separate manifest fields. An evidence-only/documentation commit does not retroactively become a tested suite. The dedicated workflow excludes Markdown, generated coverage documentation and the reports directory from its push filter, so updating a report alone does not rebuild the native/contract lanes. Shared executable inputs and registry/target locks remain trigger inputs. Existing global PR workflows are unchanged.

Evidence collection keeps content-addressed raw receipts and append-only per-attempt snapshots. Root summaries are only current aliases; prior failures, job metadata and observations are retained before aliases change. The collector stops at its per-run retention budget rather than deleting history. G-SELF-091/092 test idempotence and refusal to overwrite a before-FAIL receipt with PASS.
