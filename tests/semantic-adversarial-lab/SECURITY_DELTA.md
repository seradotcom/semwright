# Security delta — internal G preparation

## Scope and current truth

This is an internal adversarial laboratory, not an independent R16 closeout. No product vulnerability is confirmed solely from source hypotheses or an unexecuted test. See exact-SHA Actions receipts for actual execution. All native domains and the combined candidate retain explicit open coverage.

G corrected two defects in its own evidence oracle. These are laboratory reliability findings, not product security bugs. Their fixes are authored by G and must not be called independent review of G's own code. Severity below concerns the laboratory's evidence trust boundary; no host exploit or native application compromise has been demonstrated.

## G-LAB-001 — non-finite values accepted through exponent overflow

Classification: test/oracle defect. Provisional impact: medium, because a malformed measurement could enter a parser documented as strict even though `NaN` and literal `Infinity` were rejected. The prior oracle used the standard floating-point parser for finite-looking exponent syntax; overflow could yield a non-finite Python value.

Before suite: `8ec7a74c3518216feb02ba1b9c55cf3183a0da2a`, `lab_core.py::strict_json`. First fix suite: `85d39e89232c8bbf80e0fce98e097b36e6c05860`. The fix enforces finite exponent parsing, UTF-8, valid scalar strings, and bounded depth/node count. Source inspection establishes the code change, not hosted execution.

Hosted controls: G-SELF-053 loads the original oracle from the frozen Git blob and observes the old behavior inside the enclosure. G-SELF-031/032 require the new parser to reject overflow; G-SELF-033–037/049 exercise encoding and boundedness controls. The old blob digest is pinned in `targets.json`. Status: FIX_IMPLEMENTED_VALIDATION_PENDING until corresponding structured receipts exist.

## G-LAB-002 — closure accepted without an executed affected family

Classification: test/oracle defect. Provisional impact: medium. The old validator checked reported finding identity, FAIL/PASS labels, fix SHA, affected-case names and run/job presence, but not an actual complete, same-suite execution family or hashed evidence. Such a receipt could look like closure without reproducing the issue and its fix.

Before suite: `8ec7a74c3518216feb02ba1b9c55cf3183a0da2a`. First fix suite: `85d39e89232c8bbf80e0fce98e097b36e6c05860`. The new validator recomputes before and after outcomes from exact affected results, requires an identical oracle content hash across the recorded suite commits, positive observed run/job IDs, evidence SHA-256, no infrastructure blockers, verified cleanup, and consistent product/native scope.

Hosted controls: G-SELF-054 retains the before/fix contrast; G-SELF-042–048/050–052 reject fabricated or incomplete closure. G-SELF-043 is the valid positive control. Status: FIX_IMPLEMENTED_VALIDATION_PENDING until actual hosted receipts are collected. No product finding is closed by this change.

## Evidence-ingestion hardening

The read-only collector binds run/attempt/job to the explicit suite and frozen targets. It independently recomputes counts and verdicts and requires an authenticated artifact digest. Its archive reader uses exact member names, byte/entry budgets, rejects symlinks/traversal/duplicates, and reads bounded bytes without extraction. G-SELF-055–082 are hosted positive/negative controls for this lab boundary. They are not tests of the product's package installer.

## Residual risks and blocked coverage

Contract fixtures can falsify specific library guarantees, but cannot establish authenticated Broker reachability, native save/reopen, sandbox confinement of an external application, complete cross-project privacy, whole-video coverage, intelligibility, or rollback across apps. The tested-subprocess enclosure is not the product sandbox. Declared NativeApi/DecodedMedia enum values in synthetic fixtures are not independent native observations.

An additional target-source inspection was blocked by the remote tool during continuation. No alternative access route was used to bypass that block. The existing target locks were retained. New C/F and native adapters were not fabricated from illustrative APIs.

Unfixed sensitive product findings, should they be established, remain private with their owner until coordinated disclosure. Severity is assigned only after reachability and impact review. The public test package contains no real credentials, user documents, native projects from private sessions, recordings, or public network listeners.

## Continuation safeguards

Retests retain both full suite commits and a separately recomputed immutable oracle fingerprint. Changing only the target selector cannot hide changed expectations, guards, contract/runtime pins or budgets. Collection retains prior raw receipts and per-attempt reports instead of overwriting failure history. These are G-owned harness changes; their controls are included in the current registered suite and still require hosted execution. No product finding is auto-closed.
