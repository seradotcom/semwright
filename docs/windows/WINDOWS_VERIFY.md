# Windows verification

Use the exact commit being certified. Keep native CI and interactive-desktop evidence separate.

## Native CI

`.github/workflows/windows-platform.yml` runs native Windows x64 and ARM64 checks. It covers the platform crates, Driver/Plugin Hosts, secure spawn, authority profiles, UIA fixture semantics available on hosted runners, sealed-tool compatibility and portable contract regressions. A green hosted run may be labeled `PASS_WINDOWS_NATIVE_CI`; it must never be labeled `PASS_WINDOWS_INTERACTIVE`.

The Windows authority boundary currently includes platform-owned AppContainer/LPAC spawn, Job containment, owner-gated ambient network, Driver workspace/system-config/secret/sealed-tool/loopback authority and Plugin workspace mounts. Governed external MCP spawn is supported, while external MCP filesystem mounts remain `BLOCKED_PORTABLE_PATH_VIRTUALIZATION`.

## Current hosted baseline

Public run `37228279724` on source `04cf0ef7062d134b71206d832375be9545553bb4` completed successfully for native Windows x64, native Windows ARM64, `server2022-x64` sealed-tool compatibility, and `win11-arm-native-host-x64tool` compatibility. This is exact-SHA noninteractive evidence; later platform source changes must rerun the affected Windows lanes rather than inherit this result.

## Physical UIA with a hosted build

For one bounded UIA check without compiling on the interactive machine, use the
`windows-uia-physical-test` artifact from the Windows workspace connection workflow.
It contains the existing native Win32 test executable and a source/SHA-256 manifest.
The hosted job only builds this executable; it does not claim physical desktop evidence.

From a clean checkout at the artifact's exact source commit, download the artifact and
run on an unlocked disposable Windows session:

```powershell
gh run download RUN_ID --repo seradotcom/semwright --name windows-uia-physical-test --dir 'D:\SemwrightValidation\uia-bin'
pwsh ./scripts/windows/run-interactive-certification.ps1 -PrebuiltUiaDirectory 'D:\SemwrightValidation\uia-bin' -EvidenceRoot 'D:\SemwrightValidation\uia-evidence'
```

Use a new or empty evidence directory. Set process-local TEMP/TMP to an authorized
scratch directory when local data must stay on a particular drive. No Cargo invocation,
local Rust build, capture-consent prompt or self-hosted runner is needed in this mode.
The harness verifies the source and binary hash before execution and limits the process
to 45 seconds. It requires exactly one passing test, not a zero-test exit status.

The case creates its own Win32 window, inspects and invokes its button through UIA,
checks the changed text, denies reading the password control and rejects the old
reference after the window closes. Only `uia_semantic` can receive
`PASS_WINDOWS_INTERACTIVE`; the overall matrix and all unexecuted rows remain
`WINDOWS_INTERACTIVE_PENDING`. This is an actual control in a disposable fixture, not
acceptance of arbitrary applications or the complete agent workflow.

## Interactive certification

Run on an unlocked disposable Windows desktop from a clean checkout:

```powershell
pwsh ./scripts/windows/run-interactive-certification.ps1 -CaptureMode Both
```

Or manually dispatch `.github/workflows/windows-interactive.yml` to a self-hosted Windows runner. The workflow is `workflow_dispatch` only; the harness rejects noninteractive/service sessions even on self-hosted machines.

The harness writes `verification/windows-interactive/<timestamp>/result.json` plus per-row logs and SHA-256 hashes. It marks only actually executed rows as `PASS_WINDOWS_INTERACTIVE`; unexecuted rows remain `WINDOWS_INTERACTIVE_PENDING`. The overall classification remains pending while any required row is pending.

The WGC selection test requires the operator to choose a non-sensitive target in the system picker. The cancellation test requires the operator to cancel the picker. The harness does not automate consent.

Physical/machine-specific rows such as elevated-target UIPI denial, UAC secure desktop, mixed-DPI/multi-monitor, lock/unlock/sleep/wake, real application coverage and UIA virtualization/event stress remain pending until that evidence is supplied.
