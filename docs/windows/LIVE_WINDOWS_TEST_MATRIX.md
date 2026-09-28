# Live Windows 11 acceptance matrix

Hosted CI and an unlocked desktop certify different things. No row becomes `PASS_WINDOWS_INTERACTIVE` until its interactive evidence is attached to the exact commit.

| area | automated/human evidence | current status |
|---|---|---|
| UIA semantic fixture | snapshot, native hit-test, InvokePattern, Value/Text read, password redaction, stale ref | `WINDOWS_INTERACTIVE_PENDING`; source/native CI present |
| Unicode input + focus drift | ignored interactive fixture uses `input.type`, verifies exact Unicode and rejects drift with `Conflict` | `WINDOWS_INTERACTIVE_PENDING` |
| pointer | fixture-owned button click + observable mutation; relative pointer move + physical cursor change | `WINDOWS_INTERACTIVE_PENDING` |
| clipboard | Unicode write/read plus caller byte-budget denial | `WINDOWS_INTERACTIVE_PENDING` |
| capture selection | human selects a non-sensitive item in GraphicsCapturePicker; bounded PNG validated | `WINDOWS_INTERACTIVE_PENDING` |
| capture cancellation | human cancels GraphicsCapturePicker; result must be `Cancelled` | `WINDOWS_INTERACTIVE_PENDING` |
| Driver/Plugin/MCP secure spawn | rerun native secure-spawn/authority suites in same interactive checkout | `WINDOWS_INTERACTIVE_PENDING`; `PASS_WINDOWS_NATIVE_CI` already available |
| Driver workspace/system-config/secret/tool/network | native authority suites | `PASS_WINDOWS_NATIVE_CI`; interactive rerun pending |
| Plugin workspace/network | native authority suites | `PASS_WINDOWS_NATIVE_CI`; interactive rerun pending |
| external MCP network | native authority suite | `PASS_WINDOWS_NATIVE_CI`; interactive rerun pending |
| external MCP filesystem mounts | no sound transparent `/workspace/<name>` virtualization on Windows | `BLOCKED_PORTABLE_PATH_VIRTUALIZATION` |
| UIA events/virtualization/large trees | real applications/virtualized controls + bounded event stress | `WINDOWS_INTERACTIVE_PENDING` |
| Notepad / Calculator / Explorer / WinUI / Chromium | semantic real-app matrix | `WINDOWS_INTERACTIVE_PENDING` |
| UIPI | normal process -> elevated target; no bypass | `WINDOWS_INTERACTIVE_PENDING` |
| UAC secure desktop | no interaction | `WINDOWS_INTERACTIVE_PENDING` |
| display | 96/125/150/200%, mixed DPI, negative coordinates, multi-monitor | `WINDOWS_INTERACTIVE_PENDING` |
| lifecycle/session | close/recreate, PID/HWND reuse, lock/unlock, sleep/wake | `WINDOWS_INTERACTIVE_PENDING` |
| IPC negative cases | wrong SID/session, remote client, DACL inspection, impersonation reversion | `WINDOWS_INTERACTIVE_PENDING` |

## Harness

From a clean checkout on an unlocked disposable Windows desktop:

```powershell
pwsh ./scripts/windows/run-interactive-certification.ps1 -CaptureMode Both
```

A manual GitHub dispatch may use `.github/workflows/windows-interactive.yml`, which targets self-hosted Windows runners only. The harness itself additionally requires an interactive user session, Explorer in that session and a visible foreground window; a service/noninteractive runner fails closed. GitHub-hosted Windows runners cannot run the workflow.

Evidence is written under `verification/windows-interactive/` and contains per-row logs, SHA-256 hashes, commit SHA, architecture/session metadata and `result.json`. The overall result intentionally remains `WINDOWS_INTERACTIVE_PENDING` while any required row above is pending.
