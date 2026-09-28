# Windows verification

Use the exact commit being certified. Keep native CI and interactive-desktop evidence separate.

## Native CI

`.github/workflows/windows-platform.yml` runs native Windows x64 and ARM64 checks. It covers the platform crates, Driver/Plugin Hosts, secure spawn, authority profiles, UIA fixture semantics available on hosted runners, sealed-tool compatibility and portable contract regressions. A green hosted run may be labeled `PASS_WINDOWS_NATIVE_CI`; it must never be labeled `PASS_WINDOWS_INTERACTIVE`.

The Windows authority boundary currently includes platform-owned AppContainer/LPAC spawn, Job containment, owner-gated ambient network, Driver workspace/system-config/secret/sealed-tool/loopback authority and Plugin workspace mounts. Governed external MCP spawn is supported, while external MCP filesystem mounts remain `BLOCKED_PORTABLE_PATH_VIRTUALIZATION`.

## Interactive certification

Run on an unlocked disposable Windows desktop from a clean checkout:

```powershell
pwsh ./scripts/windows/run-interactive-certification.ps1 -CaptureMode Both
```

Or manually dispatch `.github/workflows/windows-interactive.yml` to a self-hosted Windows runner labeled `semwright-interactive`. The workflow is `workflow_dispatch` only and rejects non-self-hosted execution.

The harness writes `verification/windows-interactive/<timestamp>/result.json` plus per-row logs and SHA-256 hashes. It marks only actually executed rows as `PASS_WINDOWS_INTERACTIVE`; unexecuted rows remain `WINDOWS_INTERACTIVE_PENDING`. The overall classification remains pending while any required row is pending.

The WGC selection test requires the operator to choose a non-sensitive target in the system picker. The cancellation test requires the operator to cancel the picker. The harness does not automate consent.

Physical/machine-specific rows such as elevated-target UIPI denial, UAC secure desktop, mixed-DPI/multi-monitor, lock/unlock/sleep/wake, real application coverage and UIA virtualization/event stress remain pending until that evidence is supplied.
