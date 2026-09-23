# Compatibility and verification levels

This table separates implementation from evidence. A compile or cross-target check is not a live desktop certificate.

| Environment or route | Implementation boundary | Current evidence | Remaining |
|---|---|---|---|
| Rust toolchain | MSRV 1.88; development pin 1.98.1 | locked workspace CI on both policy points | raise MSRV only through an explicit reviewed change |
| Linux portable/runtime core | Provider Runtime + platform boundary | workspace fmt/check/Clippy/tests/doctests/docs and source contract gates | live desktop matrix remains separate |
| GNOME Wayland | AT-SPI + optional GJS bridge + portal | Rust/contract tests | real GNOME version/consent/window matrix |
| Plasma Wayland | AT-SPI + KWin bridge + portal | Rust/contract tests | real KWin lifecycle matrix |
| Sway / i3-style IPC | typed native socket commands/tree | Rust tests | live Sway identity/workspace/focus |
| Hyprland | native JSON socket / dispatch | Rust tests | live version-specific IPC/restart |
| Native X11 | EWMH + explicit XTEST fallback | bounded/lifecycle Rust tests; dedicated live test is opt-in | isolated Xvfb/WM execution in exact integration SHA |
| AT-SPI | dedicated accessibility bus | normalization/selector/lifecycle source + tests | GTK/Qt private-bus and live event-loss testing |
| Portal | portal provider | lifecycle/state tests | live consent/revocation and complete EIS/PipeWire evidence |
| macOS ARM64 / Intel | `platform-macos[-sys]` + Swift/C Apple bridge | portable Rust crates cross-check for both Darwin targets | native Apple-SDK CI; TCC/live Mac acceptance |
| macOS Accessibility/Input/Capture | AXUIElement / CoreGraphics / ScreenCaptureKit | source + platform-model tests only until native CI | real authorized interactive Mac |
| macOS arbitrary drivers/plugins | platform launcher boundary | deliberately unavailable | prove supported isolation model before enabling |
| Blender / LibreOffice / MLT / KiCad | first-party DriverProviders | repository-specific tests/integration gates | per-application live matrix varies |
| Chromium | private-profile CDP adapter | real hosted browser integration on Linux development line | broader OS matrix |
| Plugins | platform sandbox service | Linux bubblewrap/Landlock implementation and tests | adversarial/live sandbox matrix |
| Windows | future platform host | no implementation claim | platform host + native Windows evidence |

Bridge manifests and source availability are not support guarantees. macOS support must not be announced solely from Linux cross-compilation or hosted noninteractive tests. Windows is a future host, not an implemented fallback.
