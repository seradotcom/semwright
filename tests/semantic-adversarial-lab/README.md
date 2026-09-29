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

PR #174 contains the independent lab, not product changes. Suite `fe78c7b048d1e88f6646f28d7ac28d3b9f84c119` registers **217 cases**: 82 oracle/ingestion controls, 70 Composition/codec cases, 30 PCM/WAVE cases and 35 AV/sync cases. Registered is not executed. The latest machine-readable experiment index records observed counts and blockers; no number here asserts PASS.

Read `SECURITY_DELTA.md`, `INTEGRATION.md`, `RELEASE_IMPACT.md` and `RUNBOOK.md` for source/fix attribution, native limitations and exact-SHA collection. The archive-reader controls concern the lab's evidence ingestion, not production package installation. `COVERAGE.json` keeps all untested mandatory domains open.

The next source revision adds immutable evidence history and content-addressed oracle identity for target-only retests: **227 registered cases**, including 92 lab controls. Source identity, expectations, runtime/limit pins and evidence history are distinct from reported PASS. `COVERAGE.json` is an open implementation matrix; generated experiment receipts carry tested SHAs and actual case outcomes.
