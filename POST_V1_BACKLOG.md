# Post-v1 environment-dependent backlog

This backlog contains only the physical/interactively dependent residuals intentionally deferred from
v1 engineering closeout and the initial-v1 publication prerequisite set under
[the 2026-10-03 staging policy](docs/release-policy.md). These residuals do not block the initial
release once its engineering/security/maintainer/exact-SHA gates are satisfied. They still block
the corresponding physical/interactive support claims. Do not close a row from hosted CI, a nested/synthetic display, a screenshot,
or an operator statement without the required machine evidence.

<a id="r06"></a>
## R06 — physical Linux desktop residuals

**State:** `OPEN — DEFERRED_TO_POST_V1_ENVIRONMENT_DEPENDENT`

### Already implemented/evidenced

The repository already contains real GNOME/Plasma/Sway/X11 evidence plus hardware-backed nested
Hyprland native-socket coverage. The nested Hyprland record proves list/focus/move/resize,
stale-reference rejection, cleanup, and synthetic mixed-scale compositor state. It is not physical
login or physical mixed-DPI evidence.

### Environment still required

Use a disposable **local physical Hyprland Wayland login**. For mixed-scale certification, connect at
least two physical outputs and configure distinct scales. A nested compositor, headless output or
single-panel laptop is insufficient.

### Exact completion procedure

1. Check out the exact source SHA to certify, require a clean worktree, and record `git rev-parse HEAD`.
   Build or obtain the matching `semwright` and `semwrightd` binaries and record SHA-256 for both.
2. From the physical Hyprland login, run the fail-closed preflight:

```bash
export SEMWRIGHT_HYPR_PHYSICAL_ACK=I_AM_ON_A_DISPOSABLE_PHYSICAL_HYPRLAND_LOGIN
./scripts/dev/hyprland-physical-preflight.sh | tee r06-physical-preflight.json
```

3. For the physical mixed-scale row, connect two physical displays, set distinct scales in Hyprland,
   then require the stronger preflight:

```bash
export SEMWRIGHT_HYPR_REQUIRE_MIXED_SCALE=1
./scripts/dev/hyprland-physical-preflight.sh | tee r06-mixed-scale-preflight.json
```

   Both preflights must report `PASS_PREFLIGHT_ONLY` and `certification_complete=false`.
4. Create a disposable native Wayland fixture on the physical session. Reuse the production Semwright
   policy and CLI sequence documented in `scripts/dev/hyprland-kwin-live-cert.sh`: `doctor`,
   `window.list`, `window.focus`, `window.move`, `window.resize`, fixture exit, stale-reference
   rejection, and final cleanup. Do not create a nested KWin/Hyprland compositor or headless monitor.
5. Restart the physical Hyprland session/compositor using the normal owner-approved login lifecycle,
   then verify Semwright rediscovers backend `hyprland` without a manual state transplant. Record
   pre/post compositor PID, session identity, daemon PID, `doctor` output and a fresh `window.list`.
6. Exercise focus drift/cancellation only against the disposable fixture. A request whose target focus
   changed must fail closed; a cancelled in-flight request must produce no post-result mutation.
7. For mixed-scale/multi-monitor, record `hyprctl monitors -j` showing at least two **physical**
   connector-like outputs, distinct scales, positions (including negative coordinates if used), and
   repeat list/focus/move/resize on windows placed on each output.
8. Record `hyprctl version`, `loginctl show-session`, monitor JSON, Semwright JSON outputs, binary
   hashes, source SHA and cleanup result in a dated `verification/live-hyprland-physical/` bundle.
9. Close R06 only if every required physical row passes on the exact source. Any product failure opens
   a software finding instead of being waived as an environment issue.

### Claims withheld until completion

Do not claim certified physical Hyprland-login support or physical mixed-scale/multi-monitor support.

<a id="r18"></a>
## R18 — unlocked Windows interactive residuals

**State:** `OPEN — DEFERRED_TO_POST_V1_ENVIRONMENT_DEPENDENT`

### Already implemented/evidenced

Native Windows CI covers the implemented UIA/host/secure-spawn authority surface. The interactive
harness and self-hosted workflow are already in-tree, but hosted runners do not count as an unlocked
desktop certificate.

### Environment still required

Use a disposable Windows 11 machine/VM with a real unlocked interactive user desktop, Explorer shell,
visible foreground window, PowerShell, the pinned Rust toolchain, and the exact clean Semwright
checkout. Mixed-DPI requires at least two displays with different effective DPI. UIPI/UAC rows require
an elevated target and the actual secure desktop.

### Exact completion procedure

1. Check out the exact source SHA to certify and confirm `git status --porcelain` is empty.
2. Run the built-in interactive harness with both human WGC actions:

```powershell
pwsh -NoProfile -File .\scripts\windows\run-interactive-certification.ps1 -CaptureMode Both
```

   Alternatively dispatch `.github/workflows/windows-interactive.yml` only on a self-hosted Windows
   runner attached to that unlocked desktop. GitHub-hosted runners are invalid for R18.
3. Preserve the generated `verification/windows-interactive/<timestamp>/result.json`, row logs and
   SHA-256 log hashes. Do not edit a pending row to PASS without executing it.
4. Complete the remaining rows from `docs/windows/LIVE_WINDOWS_TEST_MATRIX.md` on the same exact SHA:
   - **UIA events/virtualization/large trees:** use real applications with virtualized controls; record
     bounded event delivery, stale-object handling and large-tree behavior.
   - **Real apps:** exercise semantic read/action paths in Notepad, Calculator, Explorer, a WinUI app
     and Chromium; record application versions and Semwright JSON results.
   - **UIPI:** from normal integrity, target an elevated test window; input/automation that Windows
     blocks must remain blocked and Semwright must not elevate or use `uiAccess`.
   - **UAC secure desktop:** trigger a UAC consent prompt; Semwright must not automate or capture the
     secure desktop. Record the negative result after returning to the normal desktop.
   - **Mixed DPI/multi-monitor:** test 96/125/150/200% combinations that are physically available,
     at least one mixed-DPI pair, negative coordinates where supported, and cross-monitor UIA/input
     coordinates. Record Windows display configuration and observed physical/logical coordinates.
   - **Lifecycle:** close/recreate targets, exercise PID/HWND reuse, lock/unlock, and sleep/wake;
     stale references must fail and reprobe must recover only current objects.
   - **IPC negatives:** wrong SID/session and remote-client attempts must be rejected; inspect the
     named-pipe DACL and verify impersonation is reverted after each request.
5. Store manual-row logs/evidence beside the harness bundle and update `result.json` only from actual
   observations. Every required row must be `PASS_WINDOWS_INTERACTIVE`; any `FAIL` is a software
   finding and any unexecuted row remains `WINDOWS_INTERACTIVE_PENDING`.
6. Close R18 only when the exact-SHA bundle is complete and independently reviewed.

### Explicit non-claim that is not converted into R18 PASS

Windows external MCP filesystem mounts remain `BLOCKED_PORTABLE_PATH_VIRTUALIZATION`. Do not claim
transparent `/workspace/<name>` mount virtualization until a separately reviewed portable design is
implemented and tested. This fail-closed unsupported surface is not hidden inside the environmental
defer.

### Claims withheld until completion

Do not claim complete unlocked-Windows interactive certification, UIPI/UAC certification, real-app
UIA certification, mixed-DPI/multi-monitor certification, or lock/wake lifecycle certification.
