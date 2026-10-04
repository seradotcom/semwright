# Compatibility and verification levels

This table separates implementation from evidence. A compile or cross-target check is not a live desktop certificate.
R's current review snapshot is `6491c0d838fa066938a494524d69ed507aa0dbe8`; the earlier
`241000c268d1bf1dc29d4e91a913097ac0d020cb` preflight retains its failed Windows result.
The entries below combine implementation with explicitly historical evidence, not fresh
execution of every route on R's snapshot. Consult [platforms](platforms.md),
[verification](../VERIFY.md) and [I's ledger](semantic-creation/INTEGRATION.md).

| Environment or route | Implementation boundary | Current evidence | Remaining |
|---|---|---|---|
| Rust toolchain | MSRV 1.88; development pin 1.98.1 | locked workspace CI on both policy points | raise MSRV only through an explicit reviewed change |
| Linux portable/runtime core | Provider Runtime + platform boundary | workspace fmt/check/Clippy/tests/doctests/docs and source contract gates | live desktop matrix remains separate |
| GNOME Wayland | AT-SPI + optional GJS bridge + portal | real GNOME Shell 46.0 Wayland semantic GTK/AT-SPI mutation, delta and stale-ref run; owner-approved keyboard+pointer `ConnectToEIS` grant/stop lifecycle; controlled Ubuntu 24.04 host acceptance with the `d442ee18` AT-SPI backport, including direct lifetime churn, normal/guarded Semwright snapshots, event-driven churn, stale-ref recovery and semantic mutation without SIGSEGV | broader Noble hardware/session and physical mixed-scale/multi-monitor matrix; historical isolated VM input and bridge-reconnect evidence is recorded separately |
| Plasma Wayland | AT-SPI + KWin bridge + portal | hosted KWin 6 Wayland mailbox lifecycle; historical isolated Plasma 5.27 VM portal_notify keyboard delivery, cancellation, focus denial and restart recovery | broader physical scaling/multi-monitor failure matrix |
| Sway / i3-style IPC | typed native socket commands/tree | hosted real headless Sway IPC lifecycle plus Rust tests | broader restart/scaling/multi-output matrix |
| Hyprland | native JSON socket / dispatch | Rust tests plus hardware-backed nested Hyprland 0.56.2/KWin 6.7.5 lifecycle: list/focus/move/resize/stale-ref/cleanup and synthetic second output at scale 1.25 | physical Hyprland login, compositor restart/focus-drift/cancellation and physical mixed-scale matrix |
| Native X11 | EWMH + explicit XTEST fallback | hosted real Openbox/EWMH session plus bounded lifecycle/cancellation tests | broader real-login, scaling and multi-monitor matrix |
| AT-SPI | dedicated accessibility bus | hosted real GTK + native Qt fixtures and real GNOME Wayland GTK run; delta/resync/stale-ref lifecycle; Noble guard and temporary `2.52.0-1build1+semwright1` backport both exercised in CI and on a controlled real GNOME host | keep the guard for unpatched Noble systems until Ubuntu ships an equivalent/newer official fix; broaden patched-host coverage beyond the current controlled machine |
| Portal | portal provider | restore-token/clipboard fixtures, EIS protocol fixture, real synthetic PipeWire frame capture and real GNOME owner-approved keyboard+pointer `ConnectToEIS` grant/stop lifecycle | historical isolated GNOME/Plasma VM keyboard delivery/cancellation and focused GNOME pointer evidence are bounded to their recorded environments; additional physical desktop coverage remains |
| macOS ARM64 / Intel | `platform-macos[-sys]` + Swift/C Apple bridge | native hosted macOS CI on Apple Silicon and Intel plus both Darwin target checks | TCC/live interactive Mac acceptance |
| macOS Accessibility/Input/Capture | AXUIElement / CoreGraphics / ScreenCaptureKit | native hosted builds/noninteractive platform checks; actual TCC-gated operation depends on an authorized desktop | real authorized interactive Mac |
| macOS arbitrary drivers/plugins | platform launcher boundary | deliberately unavailable | prove supported isolation model before enabling |
| Blender / LibreOffice / MLT / KiCad / OBS | first-party DriverProviders | repository-specific tests/integration gates | per-application live matrix varies |
| Figma | official Plugin API via authenticated loopback DriverProvider bridge | typed/plugin/fake-host tests, sandboxed host CI and historical Figma 126.5.6 driver-protocol proof at 3cd86958 | full CLI/broker and broader Design/FigJam/Motion acceptance are not established by that limited proof |
| Chromium | private-profile CDP adapter | real hosted browser integration on Linux development line | broader OS matrix |
| Plugins | platform sandbox service | Linux Bubblewrap/Landlock with executed hostile plugin/driver fixtures; platform-specific Windows authority tests | independent review and broader platform/live coverage |
| Windows | UIA, input/capture, named-pipe IPC and restricted process-launch host | run 37096430846 passed native x64/ARM64 and selected sealed-tool compatibility jobs on R's snapshot; older fixture failures remain recorded | interactive consent/capture/UIPI/session/display matrix remains open under R18 |

Bridge manifests and source availability are not support guarantees. macOS support must not be announced solely from Linux cross-compilation or hosted noninteractive tests. Windows is implemented, but a hosted job or cross-check does not imply interactive acceptance,
full Linux-equivalent isolation, or a fallback after a failed target/focus precondition.
