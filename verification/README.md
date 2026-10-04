# Verification evidence

This directory preserves source-scoped evidence from Semwright's development and release-review history.
It is an evidence archive, not the primary product documentation and not a statement that every recorded
result applies to the current `main` branch.

Historical records intentionally keep the identifiers, branch names, file paths, status vocabulary and
subsystem labels that were present when the evidence was produced. Some older records use short labels
such as A–I for parallel subsystem work. Those labels are historical provenance, not the current
maintainer model or public architecture vocabulary. Rewriting immutable evidence merely to modernize its
wording would make the record less trustworthy.

Use these files according to four rules:

1. **Evidence is exact-SHA scoped.** A PASS on one commit does not certify a later commit.
2. **Executed, skipped, blocked and not-run states stay distinct.** Missing evidence is never promoted.
3. **Historical records remain historical.** Current release policy and support language live in
   [`../docs/release-policy.md`](../docs/release-policy.md), [`../VERIFY.md`](../VERIFY.md),
   [`../RELEASE_BLOCKERS.md`](../RELEASE_BLOCKERS.md) and the current product documentation.
4. **Development coordination is not verification evidence.** Temporary prompts, scratch notes and branch
   choreography belong outside the tracked repository unless rewritten as a durable specification, ADR,
   test plan or evidence record.

The top-level files and subdirectories here may therefore contain wording that predates the current
Semwright naming and product presentation. Preserve that wording when it is part of a recorded receipt or
review package; add newer evidence alongside it rather than silently rewriting the past.
