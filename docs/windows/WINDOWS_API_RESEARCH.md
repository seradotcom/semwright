# Windows API research baseline

This document records the public API families intentionally used by the Windows implementation. It is architecture/source documentation, not interactive execution evidence.

- Microsoft UI Automation: Control View semantics, element properties, control patterns and runtime identity. UIA/COM objects stay on one dedicated actor thread rather than crossing into Tokio workers as raw interfaces.
- Input: public `SendInput`; Unicode text uses `KEYEVENTF_UNICODE`. Focus is revalidated immediately before synthetic input. UIPI refusal is propagated; Semwright never elevates or enables `uiAccess`.
- Capture: public `Windows.Graphics.Capture`, documented desktop interop for HWND-backed internal capture, `GraphicsCapturePicker` for the public consent flow, bounded D3D11 readback and bounded PNG artifacts. Hosted CI proves compilation/helper contracts; picker selection/cancellation still requires interactive Windows evidence.
- Local IPC: Win32 Named Pipes with a protected DACL, `PIPE_REJECT_REMOTE_CLIENTS`, kernel-reported client PID/session validation, scoped impersonation, `TokenUser` SID comparison and unconditional `RevertToSelf`.
- Filesystem: HANDLE identity, final-path checks, reparse/hardlink defenses and transactional AppContainer SID ACL grants. Driver and Plugin workspace mounts have native x64/ARM64 evidence. External MCP mounts remain fail-closed pending portable path virtualization.
- Identity/session: the current access-token `TokenUser` SID plus Windows session ID; client-supplied identity strings are never authoritative.
- Child containment: platform-owned `CREATE_SUSPENDED` + explicit `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` + AppContainer/LPAC `SECURITY_CAPABILITIES` + Job assignment before resume. `WindowsSandbox::command` remains denied so callers cannot bypass platform-owned spawn.
- Network: ambient network is default-deny. When both child manifest and owner gate allow it, the process receives only the required `internetClient` capability plus LPAC `registryRead` needed for Winsock initialization; direct host loopback remains separately governed.
- Resources: Job Objects provide process-tree lifetime limits and cumulative CPU accounting. Driver Host enforces per-operation CPU budgets against that accounting.
- Executable trust: SHA-256 pinning, stable HANDLE identity, PE architecture validation, owner/DACL validation and noninteractive cache-only Authenticode policy. Signing is additional evidence, never a substitute for digest/ACL policy.
- Platform paths: Windows Known Folder APIs rather than XDG/environment variables as the security root.
- CI: native Windows x64/ARM64 jobs certify noninteractive behavior. `PASS_WINDOWS_INTERACTIVE` requires the separate unlocked-desktop harness.
