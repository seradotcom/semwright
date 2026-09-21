# Wayland, portals and input

This project does not attempt to bypass compositor security. Window management is routed
through the selected compositor interface: a narrow GNOME/KWin bridge or native Sway/
Hyprland IPC. Accessibility actions use the application's AT-SPI service. Raw input is a
separate operation after authorization and an expected-focused-window check.

The delivered portal source implements CreateSession → SelectDevices → Start, tracks
consent ownership/lifecycle, closes pending requests when cancelled, watches session
closure, and uses the RemoteDesktop **Notify** keyboard/pointer methods. A session can be
used only by the issuing broker session. It does not persist or restore permission tokens.
`portal.start` with explicit keyboard/pointer flags can lead to a real user-facing chooser.

`screen.capture` uses the interactive Screenshot portal and copies a returned PNG into a
private temporary artifact with normal expiry. `screen.stream_info` probes ScreenCast
version and explicitly reports that pixel streaming is unavailable. It does **not** start
a working PipeWire stream.

**Not implemented:** EIS/libei transport, PipeWire pixel decoder, mapping of input to
stream coordinates, persistent restore tokens, and portal clipboard integration. Relative
pointer motion in the Notify route is not an assertion that monitor scaling/absolute
coordinate mapping is solved. Explicit `clipboard.*` uses a distinct helper path, not a
portal session permission inherited by accident.

Portal D-Bus source and state tests were authored but not compiled/executed here. There
was no real GNOME/KDE chooser or permission revocation run. Backends differ; an installed
portal service or a nonzero interface version alone is not sufficient proof of working
input. Use [manual-testing.md](manual-testing.md) and record the actual desktop/version.

References reviewed: the upstream [RemoteDesktop](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.RemoteDesktop.html)
and [ScreenCast](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html)
interfaces. See [research decisions](research.md) for dependency choices and deviations.
