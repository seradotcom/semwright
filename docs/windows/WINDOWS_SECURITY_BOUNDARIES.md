# Windows security boundaries

## Identity and session
The host identity is the current access-token `TokenUser` SID plus logon session ID. Desktop automation stays in the logged-in user session; no Session 0 service, SeDebugPrivilege, kernel driver, injection, UAC bypass or `uiAccess=true` is introduced.

## UIPI / UAC / secure desktop
`SendInput` is a same/lower-integrity fallback after target/focus revalidation. Refusal is returned as PermissionDenied/Conflict; Semwright never auto-elevates. Password text is redacted and generic UIA reads/writes to password controls fail closed. Real elevated-target and secure-desktop behavior remains `WINDOWS_INTERACTIVE_PENDING`.

## IPC
Named Pipes use a protected DACL for LocalSystem + the exact current-user SID, `PIPE_REJECT_REMOTE_CLIENTS`, kernel-reported PID/session validation and scoped impersonation. `TokenUser` is compared while impersonating and a drop guard always calls `RevertToSelf`. Client-provided identity strings are not authority.

## Filesystem and mounts
The generic Windows scoped-filesystem primitive remains conservative and does not claim Linux `openat2` equivalence. Sandbox workspace grants use a separate bounded model: absolute DOS paths, volume-root/UNC/device rejection, reparse and hardlink defenses, stable object identity, per-child AppContainer SID ACLs, read-only/read-write masks, transactional rollback and revocation on exit/kill/drop.

Driver workspace mounts and Plugin workspace mounts have native x64/ARM64 CI evidence. Plugin mounts are workspace-only and non-executable. Driver system-config and secret grants remain read-only/non-executable. External MCP filesystem mounts remain `BLOCKED_PORTABLE_PATH_VIRTUALIZATION`; secure MCP spawn does not imply filesystem parity.

## Executables and sealed tools
Executable verification pins SHA-256, rejects unstable/reparse/multi-link inputs, validates PE architecture and inspects owner/DACL through the verified object. Authenticode is cache-only additional evidence and never replaces digest/ACL policy. Driver sealed tools are Host-staged, re-attested, materialized inside the unique profile and executed only through the Host-mediated protocol; direct executable-path authority remains denied.

## Child containment and resources
Untrusted Driver, Plugin and governed stdio MCP children launch only through the platform-owned secure-spawn path: unique AppContainer/LPAC security capabilities, explicit inherited-handle list, suspended creation, Job assignment before resume, bounded environment/cwd and process-tree cleanup. `WindowsSandbox::command` remains denied to prevent bypassing that pre-first-instruction boundary.

Job Objects enforce lifetime process/memory/CPU constraints and expose cumulative process-tree CPU accounting. Driver Host serializes operation-level CPU accounting when a per-operation budget is configured so concurrent calls cannot charge each other.

## Network
Ambient network is default-deny. Owner-authorized `network=true` materializes only the `internetClient` network capability and LPAC `registryRead` required for Winsock initialization. Native x64/ARM64 tests prove outbound Internet reachability, offline denial, restart stability, no direct arbitrary host loopback and no filesystem-authority increase. Host-mediated loopback is a separate authority.

## Interactive evidence
Hosted native CI does not prove physical pixels, foreground input, capture-picker consent, UIPI, UAC secure desktop, mixed DPI/multi-monitor or lock/wake behavior. Those rows require the self-hosted/unlocked interactive harness and remain pending until evidence exists.
