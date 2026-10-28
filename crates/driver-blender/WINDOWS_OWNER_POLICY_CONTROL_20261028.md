# Owner-only CPU resource / mitigation control — 2026-10-28

This is a diagnostic control, not isolated Blender acceptance. P0 remains
unverified and Archipelago A05 remains BLOCKED.

The new selected test ran in the current Windows native test harness.
All five mitigation queries succeeded before and after one query-only
CreateActCtxW call on the pinned CPU DLL resource2. All flags were0 in both
snapshots. CreateActCtxW(flags8), QueryActCtxW(format1, one empty-identity
assembly) completed; ReleaseActCtx was called. CPU image bytes were unchanged.

No new LPAC process, explicit image mapping, DLL initialization, export call,
thread-context activation or policy/token change was requested by this test.
The complete observation was saved before assertions. The compiled harness
SHA256 is b050d5a993cfd9e462aac599d80aa12cbf2b19578bad475c21ae8704e809e545.

The historical LPAC preload fixture recorded system_call_disable flags4.
Those flags were not captured in the earlier CPUresource2 owner_after control.
The present run does not recover those missing historical values: it uses a
new harness process and the authorized runtime directory, not the old TempDir.
The difference keeps the FSCTL hypothesis open; it does not identify a denied
call or establish the cause of direct Blender startup failure.

Attempt01 failed during compilation because one JSON macro exceeded its
expansion recursion limit. No test executed. Attempt02 split the receipt
construction into bounded maps and passed the single exact selected test.
Both attempts and their source identities are retained.

Evidence:
[evidence index](evidence/windows-activation-20261028/owner-policy-control/INDEX.json),
[run receipt](evidence/windows-activation-20261028/owner-policy-control/attempt02-run.json),
[observation](evidence/windows-activation-20261028/owner-policy-control/owner-observation.json).

Native Linux4.5.14 remains NOT_RUN. No new runtime support, broker mutation,
GLB export, Godot handoff or artistic acceptance is claimed.

## Later same-day evidence

The follow-up disposable owner control explicitly added and confirmed flags4,
and CPU resource creation/query/release still succeeded. FSCTL alone was therefore
not sufficient to reproduce the LPAC failure. An own-fixture trace then observed
NtOpenKey(KEY_READ) denied on the system SideBySide registry key immediately
before CreateActCtxW failed. This supersedes the earlier uncertainty about the
next diagnostic target, without rewriting the earlier observation or asserting
complete causation. See WINDOWS_SXS_ACCESS_BLOCKER_20261028.md and its evidence.
A05 remains BLOCKED; no additional LPAC capability was enabled.
