# Wayland, portals and input

This project does not attempt to bypass compositor security. Window management is routed
through the selected compositor interface: a narrow GNOME/KWin bridge or native Sway/
Hyprland IPC. Accessibility actions use the application's AT-SPI service. Raw input is a
separate operation after authorization and an expected-focused-window check.

The delivered portal source implements CreateSession → SelectDevices → Start, tracks
consent ownership/lifecycle, closes pending requests when cancelled, and watches session
closure. On RemoteDesktop v2+ it attempts `ConnectToEIS` and routes granted keyboard/pointer
input through the EI/libei sender transport; if EIS is unavailable it retains the documented
RemoteDesktop **Notify** route as a compatibility fallback. A session can be used only by
the issuing broker session. Process/durable restore-token modes and portal clipboard access are
explicit opt-ins rather than inherited authority. `portal.start` with keyboard/pointer flags
uses the real user-facing compositor chooser.

`screen.capture` uses the interactive Screenshot portal and copies a returned PNG into a
private temporary artifact with normal expiry. The ScreenCast route also supports owner-scoped
sessions and bounded PipeWire frame capture; hosted native integration executes a real synthetic
PipeWire source and validates packed-frame/stride handling plus private PNG artifact output.

The EIS sender transport has an executed protocol-level test that negotiates a real
`reis` sender session and transmits keysym, UTF-8 text, relative motion, buttons and scroll.
A separate GNOME Shell 46.0 Wayland run now certifies real owner approval, portal consent,
keyboard+pointer `ConnectToEIS` negotiation, explicit stop and inactive post-stop state; see
`verification/live-portal-eis/gnome-connect-to-eis.json`. Focused input dispatch,
coordinate/scaling behavior, in-flight input cancellation and the broader desktop matrix remain
live verification work.

**Still not certified end-to-end:** mapping input to ScreenCast stream coordinates, focused
portal input across the supported desktop matrix, and multi-monitor/scaling behavior. Relative
pointer motion is not an assertion that monitor scaling or absolute coordinate mapping is solved.
Restore-token persistence, portal clipboard integration and PipeWire frame decoding are implemented
and have dedicated fixture/native evidence; they remain separate authorities rather than implicit
permission inherited from a RemoteDesktop session.

Backends differ; an installed portal service or a nonzero interface version alone is not
sufficient proof of working input. Use [manual-testing.md](manual-testing.md) and record the
actual desktop/version.

References reviewed: the upstream [RemoteDesktop](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.RemoteDesktop.html)
and [ScreenCast](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html)
interfaces. See [research decisions](research.md) for dependency choices and deviations.
