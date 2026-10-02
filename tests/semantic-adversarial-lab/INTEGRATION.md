# Integration of G's independent laboratory

G owns only `tests/semantic-adversarial-lab/` and `.github/workflows/semantic-adversarial-lab.yml`. PR #174 is a tests-only draft; no product source is patched. The original lab baseline remains `b736d41b61c4a4146c9e75c16796e251b025e69f`. A refreshed main reference is an observation, not permission to silently change an experiment target.

## Identities

`targets.json` freezes product and contract SHAs. The checked-out G commit is `LAB_SUITE_SHA`; each product checkout is independently verified as `TESTED_SOURCE_SHA`. A selftest's source is the suite itself, and `product_target_sha` is null. Neither a current branch tip nor a green run for another SHA can replace these identities.

A target update is deliberate: obtain the owner's explicit commit, inspect contract changes, update only the relevant target lock, preserve the old run and expectation registry, commit, push, and execute the relevant lane on a new run. Do not rerun an old workflow expecting it to test a newly pushed fix. No cherry-pick of product types, cross-worktree Cargo path, or private provider call is used by the lab.

## Integration candidate

There is no I candidate in the current lock. The A and B snapshots are separate experiments, not an integrated A+B build. Their results cannot certify main, C/D/E/F, or an eventual combined candidate. When I supplies a joint SHA, G must lock it deliberately, verify the affected adapters against that exact source, and execute every required domain family on that single candidate. G does not merge main or close R16.

## Execution and isolation

Push the G branch to start its registered diagnostic workflow. The matrix is an allowlist from the versioned lock, not a shell command input. Only one G boundary job is heavy at a time. Provisioning is separated from tested processes; native/contract subprocesses get synthetic fixtures, private HOME/tmp/output, cleared authority, network/PID namespaces, bounded outputs, and cleanup observation. A missing runtime or failed isolation is BLOCKED without weaker fallback.

The immutable product checkout is never edited. A separate ephemeral build copy receives a declared G example. Target guard mutants are recorded diffs applied only to that copy, each with target SHA, suite SHA and diff digest. Product fixes belong to their owners.

## Gates and review

The dedicated gate requires selected jobs to succeed. Contract failure, missing cases, failed preparation, or unverified cleanup cannot become PASS. Existing repository checks and protections are untouched. Inherited PR checks may additionally run under the repository's existing policy; G does not change them to hide failures.

Review the raw structured receipts with the read-only collector, not a substring search for PASS. Product failures remain untriaged until the owner and G distinguish an actual contract violation from a test/setup defect. A closure needs before/fix/after, identical oracle code/expectations/runtime/limit pins, the full affected family, observed job IDs and content-addressed evidence. No change to assertions merely to match observed output is acceptable.

## Current tested and open consumers

Exact-SHA G evidence now covers final Project Graph 74/74, F effect contracts 40/40, Broker routing 12/12, package/Skill hostility 20/20, Audio 30/30, AV 35/35, Figma contracts 17/17, Motion contracts 20/20, lifecycle faults 18/18 and clean-room driver distribution 12/12. These are independent experiments on their frozen product SHAs, not a synthetic combined certification.

Pinned native Godot and Blender were also exercised through real product routes. Godot is green on D FIX_SHA 70bd7857 and Blender is green on E second FIX_SHA f492f13. Composition is 70/70 PASS on A FIX_SHA 7ab43f99. All currently confirmed owner findings are closed by exact-SHA G retests. The remaining gate is an explicit I combined-candidate handoff; G will not synthesize one from separate owner SHAs.

No explicit I candidate has been supplied. An observed integration branch/certification is retained as context only and is not adopted into combined_candidate_sha. Full-wave candidate testing therefore remains BLOCKED.

## Retests across target-only commits

Changing `targets.json` creates a new Git suite commit even when the probes and expectations are unchanged. `oracle_identity.py` therefore computes an additional `oracle_tree_sha256` over executable fixtures, registry, mutants, workflow and all non-report inputs, plus contract/runtime/limit/history pins. Only target selection, selected lanes and the combined-candidate pointer are excluded. Both full suite SHAs are still recorded. The collector recomputes the fingerprint from each immutable Git tree; a report-provided matching string is not sufficient provenance.

A target-only commit may retest a FIX_SHA without weakening the oracle. Changed guards, expected outcomes, budgets or runtime/contract pins change the fingerprint and cannot close a finding under the unchanged-oracle protocol; re-establish the before/fix comparison under a deliberately reviewed new oracle revision instead. G-SELF-083–090 cover this distinction.
