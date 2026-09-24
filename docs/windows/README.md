# Semwright Windows host source drop

Baseline: `13b486aa67f89c039bc526321cccd869d1a68bd8`.

This directory describes the third-platform Windows implementation in this source drop. It is deliberately conservative: source exists for UI Automation, Win32 window/input/clipboard primitives, HANDLE-based read-only confinement, Known Folders, PE/architecture verification, owner-only Named Pipes, Job Objects and Windows.Graphics.Capture target acquisition, while arbitrary third-party Driver/Plugin execution and capture pixel readback remain fail-closed until their stronger native contracts are proven.

No build or test from this source drop was run locally. The Windows workflow is provided for the integrator to run in GitHub Actions.

Status vocabulary: `IMPLEMENTED_SOURCE` means code is present; `PASS_WINDOWS_NATIVE_CI` may only be written after native Actions succeeds; `PASS_WINDOWS_INTERACTIVE` requires an actual unlocked Windows desktop; otherwise use `WINDOWS_INTERACTIVE_PENDING`.
