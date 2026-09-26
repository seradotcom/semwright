# Compatibility and verification levels

This table separates implementation from evidence. A compile or cross-target check is not a live desktop certificate.

| Environment or route | Implementation boundary | Current evidence | Remaining |
|---|---|---|---|
| Rust toolchain | MSRV 1.88; development pin 1.98.1 | locked workspace CI on both policy points | raise MSRV only through an explicit reviewed change |
| Linux portable/runtime core | Provider Runtime + platform boundary | workspace fmt/check/Clippy/tests/doctests/docs and source contract gates | live desktop matrix remains separate |
| GNOME Wayland | AT-SPI + optional GJS bridge + portal | real GNOME Shell 46.0 Wayland semantic GTK/AT-SPI mutation, delta and stale-ref run; owner-approved keyboard+pointer `ConnectToEIS` grant/stop lifecycle; hosted contracts | Ubuntu 24.04 ships an AT-SPI/ATK lifetime bug that can terminate GNOME Shell under heavy automation; Semwright applies a conservative Noble guard and carries a reproducible backport of upstream `d442ee18`, pending patched-session certification; focused portal input/coordinate behavior, optional GJS bridge, scaling/multi-monitor matrix |
| Plasma Wayland | AT-SPI + KWin bridge + portal | hosted real KWin 6 Wayland mailbox lifecycle: discover/focus/resize/move/close/stale-ref | portal consent plus broader scaling/multi-monitor failure matrix |
| Sway / i3-style IPC | typed native socket commands/tree | hosted real headless Sway IPC lifecycle plus Rust tests | broader restart/scaling/multi-output matrix |
| Hyprland | native JSON socket / dispatch | Rust tests | live version-specific IPC/restart |
| Native X11 | EWMH + explicit XTEST fallback | hosted real Openbox/EWMH session plus bounded lifecycle/cancellation tests | broader real-login, scaling and multi-monitor matrix |
| AT-SPI | dedicated accessibility bus | hosted real GTK + native Qt fixtures and real GNOME Wayland GTK run; delta/resync/stale-ref lifecycle | Noble `at-spi2-core 2.52.0` heavy-automation crash hardening: legacy guard plus temporary upstream-fix backport; certify the patched package in disposable pressure tests and a controlled GNOME session before restoring unrestricted traversal on affected hosts |
| Portal | portal provider | restore-token/clipboard fixtures, EIS protocol fixture, real synthetic PipeWire frame capture and real GNOME owner-approved keyboard+pointer `ConnectToEIS` grant/stop lifecycle | focused input/coordinate/cancellation evidence plus additional portal-granted desktop coverage |
| macOS ARM64 / Intel | `platform-macos[-sys]` + Swift/C Apple bridge | native hosted macOS CI on Apple Silicon and Intel plus both Darwin target checks | TCC/live interactive Mac acceptance |
| macOS Accessibility/Input/Capture | AXUIElement / CoreGraphics / ScreenCaptureKit | source + platform-model tests only until native CI | real authorized interactive Mac |
| macOS arbitrary drivers/plugins | platform launcher boundary | deliberately unavailable | prove supported isolation model before enabling |
| Blender / LibreOffice / MLT / KiCad / OBS | first-party DriverProviders | repository-specific tests/integration gates | per-application live matrix varies |
| Figma | official Plugin API via authenticated loopback DriverProvider bridge | typed/plugin/fake-host tests plus sandboxed host CI | authorized disposable real-Figma Design/FigJam/Motion acceptance |
| Chromium | private-profile CDP adapter | real hosted browser integration on Linux development line | broader OS matrix |
| Plugins | platform sandbox service | Linux bubblewrap/Landlock implementation and tests | adversarial/live sandbox matrix |
| Windows | future platform host | no implementation claim | platform host + native Windows evidence |

Bridge manifests and source availability are not support guarantees. macOS support must not be announced solely from Linux cross-compilation or hosted noninteractive tests. Windows is a future host, not an implemented fallback.
