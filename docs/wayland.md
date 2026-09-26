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

The EIS sender transport has executed protocol-level tests that negotiate real `reis` sender
sessions and transmit text/keysyms, relative motion, buttons and scroll. `ei_text` remains the
preferred keyboard route when the compositor exposes it. For keycode-only `ei_keyboard` devices,
Semwright parses only the XKB keymap advertised by EIS and derives direct key/modifier sequences
from that mapping; it does not substitute the host layout or guess compose/IME sequences, and a
missing/unusable keymap remains `Unsupported`.

Real GNOME Shell 46.0 Wayland runs now certify owner approval, portal consent, keyboard+pointer
`ConnectToEIS` negotiation, focused pointer move/click against a disposable GTK4 target, exact
relative-logical delta behavior, focus-drift rejection with no fallback, explicit stop and inactive
post-stop state; see `verification/live-portal-eis/gnome-connect-to-eis.json`. GNOME's keycode-only
keyboard path is protocol-tested through the advertised XKB keymap; cancellation, sender
backpressure, pacing and bidirectional modifier-feedback handling also have deterministic protocol
coverage. Real keyboard target delivery is **not certified**: owner-active-login tests and same-login
nested GNOME tests were invalidated after EIS text reached the owner's active application despite
semantic window-focus checks.

**RemoteDesktop/EIS test isolation warning:** a nested GNOME Shell, private `HOME`, private runtime,
private Wayland socket and private session D-Bus are not a sufficient safety boundary for keyboard
injection on a logged-in host. Live keyboard certification must use a VM or genuinely independent
seat/login that cannot inject into the owner's active desktop.

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
