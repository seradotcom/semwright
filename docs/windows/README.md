# Semwright Windows host

Baseline: the exact Git commit containing this document.

The Windows host uses public Windows APIs and the shared Semwright authority model. Source is present for UI Automation, Win32 window/input/clipboard primitives, owner-only Named Pipes, Known Folders, PE/executable trust, Job Objects, Windows.Graphics.Capture picker + bounded D3D11 readback, and platform-owned AppContainer/LPAC secure spawn.

Native Windows x64 and ARM64 CI proves the currently supported noninteractive authority profiles: Driver, Plugin and governed stdio MCP secure spawn; Host-mediated loopback; owner-gated ambient network; Driver system-config/secret/sealed-tool grants; and bounded Driver/Plugin workspace mounts. Unsupported authority classes remain fail-closed. In particular, generic external MCP filesystem mounts remain `BLOCKED_PORTABLE_PATH_VIRTUALIZATION` because third-party MCPs cannot consume Semwright's Windows mount table as a transparent `/workspace/<name>` namespace.

Hosted Windows CI is not an interactive desktop certificate. Real UIA physical-pixel, SendInput, picker-consent, UIPI/UAC, mixed-DPI/multi-monitor, lock/wake and real-application evidence stays `WINDOWS_INTERACTIVE_PENDING` until captured on an unlocked disposable Windows session.

Use `scripts/windows/run-interactive-certification.ps1` directly on such a session, or manually dispatch `.github/workflows/windows-interactive.yml` to a self-hosted Windows runner carrying the `semwright-interactive` label. The workflow cannot run on GitHub-hosted runners.

Status vocabulary: `IMPLEMENTED_SOURCE` means code is present; `PASS_WINDOWS_NATIVE_CI` requires native Actions evidence; `PASS_WINDOWS_INTERACTIVE` requires evidence from an actual unlocked Windows desktop; `WINDOWS_INTERACTIVE_PENDING` means the interactive row has not been certified; `BLOCKED_*` means the authority remains deliberately unavailable.
