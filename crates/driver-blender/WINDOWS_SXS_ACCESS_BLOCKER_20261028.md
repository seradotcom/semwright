# Windows SxS activation access boundary — 2026-10-28

P0 remains BLOCKED. A05 remains BLOCKED. This checkpoint identifies a denied
resource in a first-party diagnostic fixture; it does not establish successful
Blender startup, broker integration, a typed mutation, a GLB export or Godot handoff.

## Owner scope and preserved state

The latest owner instruction is DRIVER FIRST. Continue the first-party isolated
Windows Blender 5.2.1 route, retain Linux support, and preserve all Driver Host
controls. The owner also explicitly forbids enlarging permissions to bypass a
failure. No additional LPAC capability, registry/system ACL change, runtime patch,
owner-token Blender fallback, descendant-process allowance or third-party Blender
MCP has been applied.

The reviewed Archipelago backbone is r21, not r06/r07. Its existing walking and
bridge evidence records 16/16 checks. Art is still REVISE and physical captures
are not evidence of Semwright-authored geometry. Extensive direct-bpy production
remains paused.

## Newly executed evidence

### FSCTL control

The ignored test owner_fsctl_restriction_control runs alone in a disposable
owner test process. It requires initial SystemCallDisablePolicy flags=0, queries
the pinned CPU DLL manifest resource 2, adds only DisallowFsctlSystemCalls
(flags=4) to that same process, confirms the effective value, and queries a second
new copy of the same DLL. There is no call which removes a restriction.

Both CreateActCtxW / QueryActCtxW / ReleaseActCtx observations succeeded. Original
CPU DLL bytes remained unchanged. Thus FSCTL restriction alone was not sufficient
to reproduce the observed LPAC failure. This is not a cold-cache experiment and
does not exclude interactions with other restrictions.

The first run completed with exit 0. The evidence explicitly says broker_used=false,
blender_executed=false, A05=BLOCKED. It is a control, not runtime acceptance.

### LPAC debugger trace

The first-party fixture was launched through the real Windows Host boundary.
The pre-resume receipt verifies the exact Job, owner SID, AppContainer SID, LPAC
and zero requested/effective capabilities. It then waits on its private stdin
for an explicitly selected test-only debugger rendezvous.

The installed GDB 12.1 attached only to this newly created fixture PID 37924.
It observed a fixed set of system API entries and returns during one
CreateActCtxW call. The trace completed and detached; GDB exited 0. The fixture
returned its actual failure and was waited for. Cargo exited 101 because the
existing assertion correctly rejects disagreement between LPAC and owner
activation. This failed test has not been relabeled PASS.

The trace contains 15 captured native API returns in approximately 0.797 seconds:

| Observation | Result |
| --- | --- |
| Several HKCU root opens during UI-language fallback | STATUS_ACCESS_DENIED |
| NtOpenFile on the pinned CPU DLL, access 0x1200a9 | Success |
| NtCreateSection for that image, access 0x4 | Success |
| NtOpenKey on HKLM\Software\Microsoft\Windows\CurrentVersion\SideBySide, KEY_READ 0x20019 | STATUS_ACCESS_DENIED (c0000022) |
| CreateActCtxW return after that last intercepted call | INVALID_HANDLE_VALUE; fixture reports Win32 5 / c0000022 |
| Owner control after the LPAC fixture exits | Create/query/release succeeds |

No NtFsControlFile or ALPC call was observed inside this captured context creation.
The last denied key is a concrete candidate in the propagation path. No causal
intervention has been authorized or run; this is not proof that it is the only
remaining cause in the complete Blender startup.

System stack names are export labels, not full private-symbol resolution. The
application frame unwind is incomplete. The trace is limited to the selected API
list and the one observed thread/call.

### Read-only registry descriptor check

A separate owner-side diagnostic opened only the public system key above for
read access/security-descriptor inspection in the explicit 64-bit and 32-bit views.
It did not read registry values or request writes.

DeriveCapabilitySidsFromName("registryRead") produced a capability SID present in
the key DACL with registry-read rights. The returned public capability SID and
DACL observations are recorded in sxs-registry-acl-20261028.json. No token was
modified and no capability was added to a test or Blender process.

Microsoft documents that LPAC needs registryRead to access registry keys which
ordinary AppContainers can access. This explains why adding that capability is
an authority decision rather than a neutral loader parameter.

## Source and security review

The preexisting initial-process attestation WIP has been preserved. It verifies
the actual suspended child before ResumeThread, including exact capability
SID/attribute sets and membership in the Host-owned Job. Malformed or ambiguous
token fields fail closed; a rejected attestation terminates and waits for the
owned process. Success still records application_startup_verified=false.

An independent read-only review of initial_attestation.rs and initial_lpac.rs
found no blocking pointer-bound, capability-comparison or fail-open defect. A
small follow-up requires returned token bytes to fit the exact length requested,
not just the alignment-rounded allocation.

An independent review of the FSCTL test and debug rendezvous found no permission
relaxation in those bodies. The rendezvous now also carries a file-level
Windows/test-feature cfg guard. Its parent module was already feature-gated.
The fixture executable requires windows-launch-test-fixture in Cargo.toml.
The private pipe and owning harness provide process/session control; the printed
nonce alone is not an independent authentication protocol.

The harness bounds the ready row, checks fixture PID/nonce/executable, restricts
the debugger acknowledgement to its private .local/logs directory, times out,
and kills/waits for its owned fixture on failure. No debugger surface is added
to the production runner command parser.

## Evidence and reproduction limits

[evidence index](evidence/windows-activation-20261028/sxs-access-trace/INDEX.json)
contains 25 preserved files at initial collection: command runners, raw-log
digests, trace, observations and exact source snapshots used for execution.
Private owner SID text is replaced with a SHA-256 label in curated trace files.
Raw originals remain under .local/logs and their digests remain in the index.

The source-at-execution directory is historical. Formatting and the defensive
guards described above came afterwards and are verified separately under
final-checks. Never substitute current-source hashes for the earlier runs.

- CPU DLL SHA-256: 1c1f06615a3fcca7c2757221fbafa3640e90591dbbc9d06f19f4962f5ce6194c
- Blender EXE SHA-256: 284f4041f98e113f3dc10654a7193ffaaa9bfdfec8b87fa116620a48b5f6d4cb
- Blender runtime: the owner-pinned 5.2.1 tree already staged for Archipelago.
- Native Linux 4.5.14 regression: NOT_RUN.
- First-party CPU/DLL fixtures are not a certification of Blender's GUI or engine.

## Remaining route

A first-class Host-owned initial native session is still needed to replace the
candidate nested process route. That requires sealed runtime/bootstrap authority,
a typed request, private transport, generation-bound readiness, cancellation,
lifetime ownership and audit. It does not, by itself, remove the SxS access denial.

No documented ACTCTX flag was established that avoids the denied system query.
A different bpy runtime would be a distinct profile with new binary identities
and its own proof; it has not been downloaded, installed or substituted.
No runtime manifest has been edited or removed.

The next authority decision is described in
[the bounded proposal](../../docs/blender/WINDOWS_SXS_AUTHORIZATION_PROPOSAL.md).
It is not approved or executed by this document.

## Primary documentation checked

- Microsoft, Launch an AppContainer:
  https://learn.microsoft.com/en-us/windows/win32/secauthz/implementing-an-appcontainer
- Microsoft, CreateActCtxW:
  https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-createactctxw
- Microsoft, ACTCTX:
  https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-actctxa
- Microsoft, System call disable policy:
  https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-process_mitigation_system_call_disable_policy
- Microsoft, Using side-by-side assemblies as a resource:
  https://learn.microsoft.com/en-us/windows/win32/sbscs/using-side-by-side-assemblies-as-a-resource
- Microsoft, Searching for assembly files:
  https://learn.microsoft.com/en-us/windows/win32/sbscs/searching-for-assembly-files

No upstream push, PR publication or external CI was performed.

## Final source verification

After formatting and the defensive guards, 11 selected tests passed: eight
attestation unit tests, one real Host stdio fixture, one exact Blender manifest
fixture, and one owner FSCTL control. The production session runner also passed
cargo check with no test-fixture feature requested. Both new LPAC fixture
processes were attested in their exact Job with zero capabilities. The Blender
EXE and CPU DLL hashes were unchanged. The final evidence index verifies 40
files and preserves the original 25-file collection index separately.

Commands, raw results and source identities are in
[evidence final checks](evidence/windows-activation-20261028/sxs-access-trace/final-checks/SUMMARY.json).
These results do not change P0=BLOCKED, A05=BLOCKED or Linux_4_5_14=NOT_RUN.
