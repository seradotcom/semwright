# Windows security boundaries

## Identity and session
The host identity is the current access-token `TokenUser` SID plus logon session ID. Desktop automation stays in the logged-in user session; no Session 0 service is introduced. No SeDebugPrivilege, kernel driver, injection, UAC bypass or `uiAccess=true` is used.

## UIPI / UAC / secure desktop
`SendInput` is treated as a best-effort same/lower-integrity fallback. A refusal is returned as PermissionDenied/Unavailable; the code never auto-elevates. Secure desktop and credential/password UI are out of scope and fail closed. Password text is redacted and generic UIA writes to password controls are denied.

## IPC
The Windows primitive creates a local Named Pipe with a protected DACL granting generic-all only to LocalSystem and the exact current-user SID, rejects remote clients, then validates the kernel-reported client PID/session. It impersonates only long enough to read `TokenUser`, compares SID bytes, and uses a drop guard to call `RevertToSelf` on every path. Client-provided identity strings are never authority.

## Filesystem
The initial Windows scoped filesystem is intentionally weaker in functionality, not weaker in stated security: pinned root HANDLE, reject UNC/device/extended paths, reparse points, ADS syntax, reserved DOS names, trailing dot/space names, multiple hard links and volume changes; one direct child read only; root identity checked before and after. Nested traversal and confined writes fail closed. It is explicitly not claimed equivalent to Linux openat2.

## Executables / DLLs
Executable verification pins SHA-256, rejects reparse and multi-link files, checks file identity/size/mtime stability while reading, parses PE and rejects AMD64/ARM64 host mismatch. The same already-open HANDLE is used to inspect owner/DACL: the owner must be the current user, LocalSystem or Builtin Administrators; a null DACL is rejected; mutation rights for any other principal are rejected; complex mutation ACEs are fail-closed. This policy deliberately accepts read/execute ACEs for ordinary Users/AppContainer principals.

Authenticode is additional evidence, not a replacement for the pinned digest. Verification is noninteractive and cache-only. A valid signature is accepted, an unsigned/self-contained local driver may still be accepted when digest/ACL checks pass, but an explicit Windows distrust result denies execution. DLL default search is hardened to System32/UserDirs. Immutable private staging and the secure pre-exec sandbox boundary remain required before arbitrary external drivers are enabled.

## Driver host
The existing `SandboxLauncher -> tokio::process::Command` boundary cannot prove `CREATE_SUSPENDED -> low-privilege AppContainer/LPAC token -> Job assignment -> resume` before untrusted instructions execute. Therefore Windows arbitrary Driver/Plugin Host execution is `SandboxDenied` in this source drop. This is a deliberate security result, not a missing fallback.
