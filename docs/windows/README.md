# Semwright Windows host

Baseline: `13b486aa67f89c039bc526321cccd869d1a68bd8`.

This directory describes the Windows implementation. It is deliberately conservative: source exists for UI Automation, Win32 window/input/clipboard primitives, HANDLE-based read-only confinement, Known Folders, PE/architecture verification, owner-only Named Pipes, Job Objects and Windows.Graphics.Capture target acquisition, while arbitrary third-party Driver/Plugin execution and capture pixel readback remain fail-closed until their stronger native contracts are proven.

Native Windows CI is the authoritative noninteractive verification path. Interactive certification requires an actual unlocked Windows desktop.

Status vocabulary: `IMPLEMENTED_SOURCE` means code is present; `PASS_WINDOWS_NATIVE_CI` may only be written after native Actions succeeds; `PASS_WINDOWS_INTERACTIVE` requires an actual unlocked Windows desktop; otherwise use `WINDOWS_INTERACTIVE_PENDING`.
