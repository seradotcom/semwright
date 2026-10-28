# Initial Windows process attestation — 2026-10-28

The platform now verifies the actual suspended process before ResumeThread.
This change does not establish successful Blender startup. P0 and A05 remain BLOCKED.

## Production boundary

After CreateProcessW(CREATE_SUSPENDED), the launcher assigns the child to its
kill-on-close Job, queries membership in that exact Job, and opens the child's token
with TOKEN_QUERY. It requires AppContainer, LPAC, the requested owner and container
SIDs, and the exact set of requested capability SID/attribute pairs.

The original capability request is unchanged. Identical repeated pairs are coalesced
only for comparison; contradictory attributes for one SID fail closed. This preserves
the existing potential registryRead duplicate in the ARM-emulation/network combination
without claiming that combination was run. The LPAC helper began as an exact source copy of
the existing first-party runner helper. The final defensive follow-up also bounds
the returned token length by the exact requested size; the kernel-attribute
identity rule is unchanged.

Pointer-bearing token fields are bounded within their own allocation, capped at
64 KiB and 64 capability entries. The receipt hashes SIDs instead of exposing their
raw identity strings. A successful pre-resume receipt explicitly says resumed=false
and application_startup_verified=false.

A rejected attestation terminates the owned Job/process and waits up to five seconds
for that exact process. An unconfirmed cleanup is a separate failure. No rejected
process is resumed by this branch.

## Selected physical checks

All commands ran locally on the Windows laptop, offline and locked, with toolchain
1.98.1-x86_64-pc-windows-gnu. Source hashes were recorded before and after each run.

| Check | Result | Observation |
| --- | --- | --- |
| Selected library tests | 8 PASS | Exact LPAC claim, malformed/bounded fields, valid two-SID fixture, identity/attribute/extras checks, ordinary owner negative, unrelated Job negative |
| Existing stdio fixture | 1 PASS | PID 32164 attested LPAC, exact Job and zero capabilities before resume; stdio exchange and wait completed |
| Existing network-capability fixture | 1 PASS | PID 30856 attested LPAC, exact Job and the two existing requested pairs, attributes 4; stdio and wait completed |

The network fixture performs no sockets, DNS or HTTP. Its network=true result checks
the internetClient SID. The platform's pre-resume attestation additionally verifies
all requested pairs, including the existing registryRead request. No capability was
added to the Blender route.

The native Linux 4.5.14 regression and ARM-emulated/network case remain NOT_RUN.
These selected checks do not substitute for either native lane, real Blender startup,
broker integration, typed scene mutation, GLB export or Godot readback.

## Evidence and preserved corrections

Raw logs, receipts, source snapshots, command runners and hashes are in:

crates/driver-blender/evidence/windows-activation-20261028/initial-process-attestation/

The first application script stopped at an end-of-file newline assertion before
writing production sources. Its input snapshot and script remain preserved; the
second application checks those hashes before applying the patch.

The first stdio receipt collector assumed JSON key order and missed an object that
was present in the raw log. The raw log and original receipt are unchanged. A separate
extraction receipt recovers that exact object by its first brace and references the
log SHA-256; it did not run the fixture again. The capability run used the corrected
collector. This correction affects collection, not the test result or native control.

An independent source review checked the pointer bounds, duplicate handling, exact
Job query, fail-closed cleanup and the pre-ResumeThread order. That review itself did
not execute tests. The executable results above come from the preserved native runs.

No upstream publication was performed.

## Final driver-first checkpoint verification

After the exact requested-length guard and source formatting, the eight selected
attestation unit tests and the existing zero-capability stdio fixture were run
again and passed. The exact Blender manifest first-party fixture also passed
under attested LPAC/Job with zero capabilities. This remains a manifest fixture,
not Blender execution. The owner FSCTL control passed and the production runner
checked successfully without the test-fixture feature. Eleven selected tests
passed in total; the two newly created LPAC processes were both attested with
zero capabilities. The pinned Blender EXE and CPU DLL hashes were unchanged.

See ../driver-blender/evidence/windows-activation-20261028/sxs-access-trace/final-checks/
for commands, output, source hashes, receipts and the separate formatting record.
The earlier execution snapshots and receipts above remain historical and intact.

A new independent review read the complete token and LPAC helpers and found no
blocking bounds or capability-comparison issue. Its minor exact-length suggestion
is applied. The final native tests, not the review, establish the observations.
The SxS registry access boundary remains unresolved; see
../driver-blender/WINDOWS_SXS_ACCESS_BLOCKER_20261028.md.
