# Proposed bounded SxS capability experiment

Status: OWNER AUTHORIZED on 2026-10-28; execution results recorded separately.
See WINDOWS_SXS_AUTHORIZATION_20261028.md for the explicit approval and scope.
The proposal below is preserved as the reviewed experiment; it is not a production grant.

## Decision requested

Authorize one disposable first-party diagnostic experiment with the single named
Windows capability registryRead, while retaining LPAC and the existing Host Job,
file grants, owner/runtime identity checks, private stdio and child-process ban.

The owner currently prohibits enlarging permissions to bypass the failure.
This proposal asks for a narrow exception to investigate a now-observed OS
dependency. No such exception is inferred from general authorization to implement.

## Why this is now the next useful experiment

An attested LPAC fixture with zero capabilities can read and map the pinned
Blender CPU DLL, but CreateActCtxW encounters NtOpenKey(KEY_READ) denied on:

HKLM\Software\Microsoft\Windows\CurrentVersion\SideBySide

The owner control succeeds. A control with DisallowFsctlSystemCalls enabled also
succeeds. Read-only DACL inspection confirms that the denied key grants read to
the SID returned for registryRead. These observations are preserved with hashes
in the sxs-access-trace evidence directory.

Microsoft documents registryRead as an LPAC registry-access capability. This is
a supported authority mechanism, but it is not restricted to the single key
above: the process could read other registry locations whose ACLs grant that
capability, subject to the remaining token/ACL checks. It does not inherently
grant registry writes or network access. The capability lasts for the test
process lifetime; it cannot honestly be described as available only during one
loader call.

## Exact proposed execution

1. Add an explicitly selected test-only request for registryRead alone. Do not
   use network=true, which requests additional network authority. Do not expose
   an arbitrary capability-name/SID list or change production defaults.
2. Verify the request and actual initial token match exactly: LPAC remains true,
   one named capability with expected attributes, the same owner/container
   identities and the same Host-owned Job. Reject before ResumeThread otherwise.
3. Run the existing CPU resource activation fixture with identical pinned bytes,
   private workspace, bounded output and a 15-second application deadline.
   Compare the result with the preserved zero-capability baseline.
4. Only if that fixture succeeds, launch the already pinned Blender 5.2.1 EXE
   as the initial Host-owned process with --version, a new private working
   directory, the same single capability and a 30-second application deadline.
   Capture banner, exit status, token/Job receipts and exact runtime hashes.
5. Wait for every owned process, confirm termination, preserve logs and summarize
   the observation. A timeout, cleanup ambiguity or different identity fails.
   Stop rather than adding another capability if another dependency is denied.

No .blend file, untrusted scene, plugin, script, GUI instance or broker credential
is supplied to this proposed experiment. No system ACL, global policy, installed
runtime byte, original project, external repository or existing owner process
is changed. Any test source is confined to the existing authorized contribution
branch and is explicitly test-gated.

## Acceptance and limits

A positive result would establish whether this named capability changes the
observed activation outcome and whether the fixed --version process starts.
It would not establish complete Blender functionality or A05.

This approval would not make registryRead a permanent production capability.
A production profile would still need an explicit owner-visible contract,
policy binding, audit and negative tests; no generic permission broadening is
authorized. The first-class Host native-session work, real broker mutation,
readback, GLB and Godot chain, and Linux 4.5.14 native regression remain required.

If this exception is not approved, the current zero-capability route remains
BLOCKED. Continuing with package/runtime alternatives would require a separately
identified profile and proof; swapping binaries or weakening controls silently
is not an alternative.

## Review references

- [Observed boundary and evidence](../../crates/driver-blender/WINDOWS_SXS_ACCESS_BLOCKER_20261028.md)
- [Preserved evidence index](../../crates/driver-blender/evidence/windows-activation-20261028/sxs-access-trace/INDEX.json)
- Microsoft LPAC guidance:
  https://learn.microsoft.com/en-us/windows/win32/secauthz/implementing-an-appcontainer
