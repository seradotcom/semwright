# Security delta — adversarial lab

## Scope and current truth

This is an adversarial laboratory, not an independent R16 closeout. No product vulnerability is confirmed solely from source hypotheses or an unexecuted test. See exact-SHA Actions receipts for actual execution. Contract, lifecycle, package/distribution and pinned Godot/Blender native experiments have now executed. All currently confirmed product findings are closed by exact-SHA fix retest; the combined candidate remains absent and therefore readiness remains BLOCKED.

The lab corrected two defects in its own evidence oracle. These are laboratory reliability findings, not product security bugs. Those fixes are changes to the lab itself and must not be called independent review of the same lab code. Severity below concerns the laboratory's evidence trust boundary; no host exploit or native application compromise has been demonstrated.

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

## Current product findings and residual risks

Confirmed product findings from executed exact-SHA families:

- G-FIND-COMPOSITION-001 is CLOSED_RETEST_PASS: Composition FIX_SHA 7ab43f99f4cc62be2a9b0ce9ce1155283a429768 binds permits to private vault/root identities; lab retest run 36938854785 passed 70/70 Composition including G-PLAN-022/023/024.
- G-FIND-GODOT-001 and G-FIND-GODOT-002 are CLOSED_RETEST_PASS on Godot FIX_SHA 70bd7857: Godot native 13/13 PASS.
- G-FIND-BLENDER-001 is CLOSED_RETEST_PASS on Blender second FIX_SHA f492f13: Blender native 14/14 PASS; G-BLENDER-010 now preserves exact managed source projection while producing the valid hashed GLB and preserving the external sentinel.

Godot and Blender findings were reached through pinned real native runtimes and product routes. Their product fixes were retested by the lab on the exact affected families and are now closed; the lab did not patch product implementation branches.

Clean contract fixtures still do not establish every live collaboration, OS, device, media-intelligibility or cross-app property. The lab enclosure is Linux/GitHub-hosted only and is not itself the product sandbox. Separate subsystem exact-SHA results cannot be combined into a final integration certification. No explicit combined candidate was supplied to this lab checkpoint.

The public test package contains no real credentials, user documents, private native projects, recordings or public network listeners. Residual risk and severity remain bounded by the tested evidence rather than inferred from green implementation branches.

## Continuation safeguards

Retests retain both full suite commits and a separately recomputed immutable oracle fingerprint. Changing only the target selector cannot hide changed expectations, guards, contract/runtime pins or budgets. Collection retains prior raw receipts and per-attempt reports instead of overwriting failure history. These are lab-owned harness changes; their controls are included in the current registered suite and still require hosted execution. No product finding is auto-closed.

## G-LAB-003 — missing failed-preflight diagnostics

Classification: laboratory diagnostic defect, provisional low severity. The fail-closed decision was preserved, but a nonzero probe exit discarded its captured structured stdout and left an empty error detail. The first hosted example is run 36506309475 / job 109210343273 on suite fe78c7b048d1e88f6646f28d7ac28d3b9f84c119, with zero registered cases executed and 82 marked BLOCKED. This does not establish a product sandbox defect.

The lab-only correction retains bounded diagnostic bytes/hashes and parsed failed-control names before refusal, and records them through cleanup. It does not remove namespaces, mounts, capability drops, canaries, budgets, environment filtering or cleanup checks. The narrow follow-up is selftest-only. The underlying preflight cause and the correction require a new exact-SHA job.
