# Compatibility and verification levels

This table separates implementation from evidence. A compile or cross-target check is not a live desktop certificate.

| Environment or route | Implementation boundary | Current evidence | Remaining |
|---|---|---|---|
| Rust toolchain | MSRV 1.88; development pin 1.98.1 | locked workspace CI on both policy points | raise MSRV only through an explicit reviewed change |
| Linux portable/runtime core | Provider Runtime + platform boundary | workspace fmt/check/Clippy/tests/doctests/docs and source contract gates | live desktop matrix remains separate |
| GNOME Wayland | AT-SPI + optional GJS bridge + portal | real GNOME Shell 46.0 semantic GTK/AT-SPI lifecycle plus user-approved ConnectToEIS keyboard+pointer connect/stop | portal cancellation, focus/coordinate behavior, optional GJS bridge and scaling/multi-monitor matrix |
| Plasma Wayland | AT-SPI + authenticated KWin bridge + portal | hosted KWin 6 virtual-Wayland lifecycle: discover/focus/resize/move/close + stale ref | portal consent plus broader scaling/multi-monitor matrix |
| Sway / i3-style IPC | typed native socket commands/tree | hosted real Sway 1.9/wlroots headless lifecycle using native Semwright IPC | broader scaling/multi-monitor and failure matrix |
| Hyprland | native JSON socket / dispatch | Rust tests plus retained hosted startup diagnostics | live IPC/restart on a runner/session satisfying Aquamarine dmabuf/DRM requirements |
| Native X11 | EWMH + explicit XTEST fallback | hosted Openbox-managed Xvfb EWMH lifecycle plus bounded identity tests | broader physical desktop/WM matrix |
| AT-SPI | dedicated accessibility bus | hosted real GTK + native Qt fixtures and real GNOME Wayland GTK run; delta/resync/stale-ref lifecycle | additional desktop/toolkit failure matrices |
| Portal | portal provider | restore-token/clipboard fixtures, real synthetic PipeWire capture, EIS protocol fixtures and real GNOME user-approved ConnectToEIS connect/stop | cancellation + focus/coordinate behavior and broader desktop consent/revocation matrix |
| macOS ARM64 / Intel | `platform-macos[-sys]` + Swift/C Apple bridge | native hosted macOS CI on Apple Silicon and Intel plus both Darwin target checks | TCC/live interactive Mac acceptance |
| macOS Accessibility/Input/Capture | AXUIElement / CoreGraphics / ScreenCaptureKit | source + platform-model tests only until native CI | real authorized interactive Mac |
| macOS arbitrary drivers/plugins | platform launcher boundary | deliberately unavailable | prove supported isolation model before enabling |
| Blender / LibreOffice / MLT / KiCad / OBS / Godot | first-party DriverProviders | repository-specific tests/integration gates, including real native-runtime jobs where available | per-application live matrix varies |
| Figma | official Plugin API via authenticated loopback DriverProvider bridge | typed/plugin/fake-host tests plus sandboxed host CI | authorized disposable real-Figma Design/FigJam/Motion acceptance |
| Chromium | private-profile CDP adapter | real hosted browser integration on Linux development line | broader OS matrix |
| Plugins | platform sandbox service | Linux bubblewrap/Landlock implementation and tests | adversarial/live sandbox matrix |
| Windows | future platform host | no implementation claim | platform host + native Windows evidence |

Bridge manifests and source availability are not support guarantees. macOS support must not be announced solely from Linux cross-compilation or hosted noninteractive tests. Windows is a future host, not an implemented fallback.
