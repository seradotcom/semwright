# Windows ARM64 UIA fixture disposition

Frozen product: `cd518748f742025a251b78028613aa1b16919e73`.

The original global Actions run `37074828959` remains **FAILURE**. Of 69 jobs,
62 succeeded, six were explicitly scoped out, and job `111090595445`
(`windows / windows-arm64 native noninteractive`) failed. Its UIA fixture kept
an old semantic snapshot during a bounded search for a fixture-owned physical
pixel. On the hosted, occluded ARM64 desktop, its fallback reused that old
reference. Production correctly rejected it as `StaleReference`.

The correction reacquires the semantic snapshot after that search. It changes
only the test fixture: no production authorization, stale-reference check,
native assertion, time budget, or required job step is removed or weakened.

The complete original ARM64 job, plus the isolated original UIA control, passed
in run `37085834067`, suite `27924ecf3bba369e0560c6685693bac3aa1dcdc3`, on the
same frozen production checkout. The artifact independently binds source and
suite; tracked production source remains unchanged. The original control passed
once, and every original native/runtime/security/check/clippy/docs step passed.
This does not certify interactive Windows or unoccluded physical-pixel access.

The explicit hosted disposition run `37088550848` rechecked the failed global
job inventory, all original ARM64 step names against the successful full retest,
the two-file external harness boundary, artifact digests and 14 current-source
certificates. Its success applies to that explicit disposition; it does not
rewrite the original global result or count skipped jobs as executed tests.

The corrected fixture is **not in the frozen product tree**. The review ZIP
carries it under `evidence/windows-arm64-corrected-fixture/`, with its generator
and hosted workflow. To reproduce, inspect the existing immutable suite and run
its `semantic-creation-integration.yml` manual workflow with
`candidate_sha=cd518748f742025a251b78028613aa1b16919e73` on branch
`ci/i-windows-arm64-freshness-cd51874`. Do not automatically retry until green.
All compilation and native execution stay on the hosted runner.

Before promoting standard CI, deliberately incorporate the test fixture
correction and certify that resulting revision's affected lane. That produces
a new candidate/suite identity; the current evaluation freeze cannot silently
move. No standard required check has been silenced, no main merge has occurred,
and R16 remains independent and open.
