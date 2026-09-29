# Semantic adversarial lab

Internal independent G lab. Start with `AUDIT_PLAN.md` and `targets.json`.

The source SHA identifies product code; suite SHA identifies the test code. For a lab selftest, source SHA equals suite SHA and product_target_sha is null. A green selftest is NOT product acceptance.

On a disposable GitHub-hosted runner with the scoped namespace prerequisites installed:

```sh
python3 tests/semantic-adversarial-lab/runner.py selftest
```

Never run attacks or builds on the workstation. Local `runner.py matrix` only reads the versioned selector. Push the branch to test new source; an Actions rerun keeps the original SHA/ref. Missing runtimes/isolations/cases fail closed. Output artifacts are uploaded even after failures; the gate does not forgive failed or skipped jobs. One heavy lane at a time.

No product source or shared workflow is changed. Compiler test overlays, product-target guard mutants, and native acceptance are different evidence classes. The initial matrix is NOT_RUN; no joint candidate or full-wave readiness is asserted.
