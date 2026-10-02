# Semantic adversarial lab

Internal independent G lab. Start with `AUDIT_PLAN.md` and `targets.json`.

The source SHA identifies product code; suite SHA identifies the test code. For a lab selftest, source SHA equals suite SHA and product_target_sha is null. A green selftest is NOT product acceptance.

On a disposable GitHub-hosted runner with the scoped namespace prerequisites installed:

```sh
python3 tests/semantic-adversarial-lab/runner.py selftest
```

Never run attacks or builds on the workstation. Local `runner.py matrix` only reads the versioned selector. Push the branch to test new source; an Actions rerun keeps the original SHA/ref. Missing runtimes/isolations/cases fail closed. Output artifacts are uploaded even after failures; the gate does not forgive failed or skipped jobs. One heavy lane at a time.

No product source or shared workflow is changed. Compiler test overlays, product-target guard mutants, and native acceptance are different evidence classes. The initial matrix is NOT_RUN; no joint candidate or full-wave readiness is asserted.

## Continuation checkpoint

PR #174 remains an independent tests-only lab. The current suite registers 480 cases across 14 lanes, including 105 hosted lab selftests. Registered case count is not itself acceptance; exact product/source SHA, suite SHA, run/job identity, receipt hash and artifact hash remain the evidence boundary.

Executed clean scopes include Project Graph 74/74, Effects 40/40, Broker routing 12/12, Skill/package attacks 20/20, Audio 30/30, AV 35/35, Figma 17/17, Motion 20/20, lifecycle 18/18 and clean-room driver distribution 12/12. Native Godot and Blender were also exercised on pinned real runtimes. Their confirmed findings are now closed by exact-SHA owner fix retests; the reports directory preserves the before/fix/after evidence.

Composition is 70/70 PASS on A FIX_SHA 7ab43f99; Godot is 13/13 PASS on D FIX_SHA 70bd7857; Blender is 14/14 PASS on E second FIX_SHA f492f13. G-FIND-A-001, D-001, D-002 and E-001 are all CLOSED_RETEST_PASS.

There is still no explicit combined I candidate in targets.json; an observed integration branch is not silently adopted. Consequently full-wave readiness and R16 remain BLOCKED even though many individual exact-SHA families are clean.

Read SECURITY_DELTA.md, INTEGRATION.md, RELEASE_IMPACT.md, RUNBOOK.md and COVERAGE.json for the exact tested/blocked split. Source identity, expectations, runtime/limit pins, oracle identity and immutable evidence history remain distinct from reported PASS.
