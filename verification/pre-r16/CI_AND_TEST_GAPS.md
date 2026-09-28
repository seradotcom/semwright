# CI and test gaps

## Exact initial SHA

`241000c268d1bf1dc29d4e91a913097ac0d020cb`. Twelve workflows completed; eleven succeeded and one failed.

| Workflow | Run | State | Conclusion |
|---|---|---|---|
| Plasma Wayland live | `36394993404` | completed | success |
| Native X11 EWMH live | `36394993501` | completed | success |
| Driver session continuity | `36394993500` | completed | success |
| Platformization and Windows | `36394993424` | completed | failure |
| Packaging certification | `36394993361` | completed | success |
| Dependency, coverage and fuzz gates | `36394993383` | completed | success |
| Platformization and macOS | `36394993418` | completed | success |
| Quality gates | `36394993428` | completed | success |
| Supply-chain certification | `36394993370` | completed | success |
| Godot driver integration | `36394993332` | completed | success |
| OBS driver integration | `36394993299` | completed | success |
| Native application integration | `36394993321` | completed | success |

Full jobs/steps are retained in `inventory/run-*-jobs.json`. Required gate definitions live in
`.github/workflows/ci.yml`, `security.yml`, native/platform/driver and release workflows.

## Failure requiring reconciliation

Windows run 36394993424, ARM64 job 108839165329 failed
`real_win32_fixture_exercises_uia_without_pixel_fallback`: exact UIA inspection returned
StaleReference after an occlusion limitation message. The later assertion is still a test failure;
the preceding noninteractive limitation does not turn it into a pass. Later steps in that job
were skipped. x64 and both compatibility jobs succeeded, but cannot substitute for this ARM64 gate.

## Positive execution evidence

`inventory/hostile-precheck-execution.txt` comes from run 36394993321, job 108839164968.
The log shows 3/3 plugin-hostile tests, 1/1 driver-hostile, 1/1 protocol-v2 and 7/7 federation tests,
with zero ignored cases. Feature `test-tools` and sandbox opt-ins were enabled for the hostile suites.
These are initial-SHA PRECHECK results, not independent review or results for the new fix commit.

## Tests executed by this audit

The targeted Python packet suite ran 18 tests in 2.755 seconds: twelve new disposable-repository
bundle regressions and six existing security-review contract tests. All passed.
Shell syntax, `cargo fmt --all -- --check`, `git diff --check` and OSS hygiene passed for the code
commit. These checks do not compile Rust, build a new target directory or exercise a live desktop.

New Rust unit/CLI/broker regressions are present but their compilation and execution are pending
Actions. Workspace check/clippy/tests/doctests, dependencies, coverage and bounded fuzz must be
observed on the eventual exact candidate SHA. No result is inferred from merely adding a test.

## Gaps

PR153's reviewed harness accepts exit-zero without requiring positive test counts and selects a
generic self-hosted Windows runner. Both points were reported to its owner. PR154 has failing
checks and mixes runtime authority remediation with GLB feature expansion. PR155 is deferred.
No interactive Windows workflow was dispatched by the audit. No physical R06 or full-runtime soak
was executed. Whole-history secret scanning remains unexecuted; narrow tracked-pattern scanning
and an empty GitHub open-alert listing are not substitutes.

## Admission rules

QUEUED, IN_PROGRESS, SKIPPED and CANCELLED are not PASS. A successful workflow that never selected
a relevant test is not execution evidence. Do not retry a failure without inspecting its root cause.
The official candidate bundle and main freeze remain withheld until required changes are integrated,
the exact main SHA is green, claims are reconciled and unique security-related work is disposed.

## Follow-up audit checks

The added builtin provenance parity test passed (1 test, 0.005s). Together with the 18 targeted
packet tests, 19 targeted Python cases passed; these are distinct from the hosted Rust suite.
The source verifier passed again with rust_compiled=false. PR156 dispatches heavy validation.
Its initial head 4c7c296 reproduced the inherited Windows ARM64 fixture failure; see FOLLOWUP_FINDINGS.md.
