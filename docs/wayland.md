# Wayland, portals and input

This project does not attempt to bypass compositor security. Window management is routed
through the selected compositor interface: a narrow GNOME/KWin bridge or native Sway/
Hyprland IPC. Accessibility actions use the application's AT-SPI service. Raw input is a
separate operation after authorization and an expected-focused-window check.

The portal backend implements CreateSession → SelectDevices → Start, tracks consent
ownership/lifecycle, closes pending requests when cancelled and watches session closure.
After a successful Start it prefers RemoteDesktop v2 `ConnectToEIS`; a negotiated EIS
session uses EI exclusively, while the documented Notify methods remain the fallback only
when EIS was not established. A session can be used only by the issuing broker session.
It does not persist or restore permission tokens. `portal.start` with explicit
keyboard/pointer flags can lead to a real user-facing chooser.

`screen.capture` uses the interactive Screenshot portal and copies a returned PNG into a
private temporary artifact with normal expiry. `screen.stream_info` probes ScreenCast
version and explicitly reports that pixel streaming is unavailable. It does **not** start
a working PipeWire stream.

The EIS sender transport is implemented with `reis` and has an executed peer-protocol test
covering negotiation, keysym/text, relative motion, button, scroll and disconnect lifecycle.
The current GNOME/Wayland host was also observed exposing RemoteDesktop v2 and
`ConnectToEIS` without starting a consent session. **Still pending:** a user-approved live
portal→EIS run, PipeWire pixel decoding, mapping stream regions to input coordinates,
persistent restore tokens and portal clipboard integration. Relative pointer motion does
not assert that monitor scaling/absolute coordinate mapping is solved. Explicit
`clipboard.*` remains a distinct helper path.

The Rust backend and EIS protocol tests compile and execute. There is still no accepted
live GNOME/KDE chooser, portal-granted EIS, or permission-revocation run. Backends differ;
an installed portal service, `ConnectToEIS` method or nonzero interface version alone is
not sufficient proof of working input. Use [manual-testing.md](manual-testing.md) and
record the actual desktop/version.

References reviewed: the upstream [RemoteDesktop](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.RemoteDesktop.html)
and [ScreenCast](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html)
interfaces. See [research decisions](research.md) for dependency choices and deviations.
