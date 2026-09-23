# Wayland, portals and input

This project does not attempt to bypass compositor security. Window management is routed
through the selected compositor interface: a narrow GNOME/KWin bridge or native Sway/
Hyprland IPC. Accessibility actions use the application's AT-SPI service. Raw input is a
separate operation after authorization and an expected-focused-window check.

The delivered portal source implements CreateSession → SelectDevices → Start, tracks
consent ownership/lifecycle, closes pending requests when cancelled, and watches session
closure. On RemoteDesktop v2 it attempts `ConnectToEIS` and routes granted keyboard/pointer
input through the EI/libei sender transport; if EIS is unavailable it retains the documented
RemoteDesktop **Notify** route as a compatibility fallback. A session can be used only by
the issuing broker session. It does not persist or restore permission tokens. `portal.start`
with explicit keyboard/pointer flags can lead to a real user-facing chooser.

`screen.capture` uses the interactive Screenshot portal and copies a returned PNG into a
private temporary artifact with normal expiry. `screen.stream_info` probes ScreenCast
version and explicitly reports that pixel streaming is unavailable. It does **not** start
a working PipeWire stream.

The EIS sender transport has an executed protocol-level test that negotiates a real
`reis` sender session and transmits keysym, UTF-8 text, relative motion, buttons and scroll.
That does **not** certify the full compositor/portal path: a real portal-granted
`ConnectToEIS` session, revocation, focus/coordinate behavior and desktop matrix remain live
verification work.

**Not implemented:** PipeWire pixel decoding, mapping of input to ScreenCast stream
coordinates, persistent restore tokens, and portal clipboard integration. Relative pointer
motion is not an assertion that monitor scaling or absolute coordinate mapping is solved.
Explicit `clipboard.*` uses a distinct helper path, not a portal session permission inherited
by accident.

Backends differ; an installed portal service or a nonzero interface version alone is not
sufficient proof of working input. Use [manual-testing.md](manual-testing.md) and record the
actual desktop/version.

References reviewed: the upstream [RemoteDesktop](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.RemoteDesktop.html)
and [ScreenCast](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html)
interfaces. See [research decisions](research.md) for dependency choices and deviations.
