# Windows API research baseline

This document records the public API families intentionally used by the source drop. It is not native execution evidence.

- Microsoft UI Automation: Control View semantics, element properties, control patterns and runtime identity. UIA/COM objects stay on one dedicated actor thread rather than crossing into Tokio workers as raw interfaces.
- Input: public `SendInput`; Unicode text uses `KEYEVENTF_UNICODE`. UIPI refusal is propagated; the implementation never elevates or enables `uiAccess`.
- Capture: public `Windows.Graphics.Capture` plus documented desktop interop for obtaining a `GraphicsCaptureItem` from a known HWND. Target acquisition source is present; bounded D3D11 single-frame readback remains pending/fail-closed.
- Local IPC: Win32 Named Pipes with an explicit protected DACL, `PIPE_REJECT_REMOTE_CLIENTS`, `GetNamedPipeClientProcessId`, `ImpersonateNamedPipeClient`, `OpenThreadToken`, `TokenUser`, SID comparison, and unconditional `RevertToSelf` through a guard.
- Filesystem: `CreateFileW`/HANDLE identity, `FILE_FLAG_OPEN_REPARSE_POINT`, `GetFileInformationByHandle[Ex]`, and final-path checks. This is deliberately **not** described as equivalent to Linux `openat2`.
- Identity/session: access-token `TokenUser` SID plus Windows session ID; client-supplied identity strings are never authoritative.
- Driver containment: Job Objects are implemented as a primitive, but arbitrary driver spawn remains fail-closed until a shared secure pre-exec spawn contract can represent token/AppContainer-or-LPAC attributes, explicit inherited handles, Job assignment and resume.
- Executable trust: digest pinning + PE format/native machine + file stability are source-present. Owner/DACL policy and Authenticode remain blockers; signing never substitutes for digest/owner policy.
- Platform paths: Windows Known Folder APIs rather than XDG or environment variables as the security root.
- CI: native Windows x64 and ARM64 jobs are defined. Hosted native CI is categorically distinct from real interactive desktop acceptance.
