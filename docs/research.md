# Targeted research and implementation decisions

Research/access date: **2026-09-21**. This is a source-selection note, not a benchmark or
competitive superiority report. URLs point to upstream documentation/projects. Documentation
being accessible does not mean its client code compiled or the API ran in this environment.
No third-party repository is vendored as this project's implementation.

| Area | Primary source consulted | Decision and limit |
|---|---|---|
| MCP | [Official Rust SDK](https://github.com/modelcontextprotocol/rust-sdk), [rmcp 3.4.0](https://docs.rs/rmcp/3.4.0/rmcp/) | Official SDK frontend, stdio, small tool surface; API signatures checked, compile unverified |
| D-Bus | [zbus 5.19.0](https://docs.rs/zbus/5.19.0/zbus/) | Tokio runtime feature; shared narrow native proxy code |
| Portal abstraction | [ashpd](https://docs.rs/ashpd/latest/ashpd/) | Current documentation showed 0.13.13; evaluated, not added to this dependency graph |
| Portal input | [RemoteDesktop interface](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.RemoteDesktop.html) | Raw zbus lifecycle plus RemoteDesktop v2 ConnectToEIS with Notify fallback only before EIS establishment; persistence remains pending |
| Portal streams | [ScreenCast interface](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html) | Version probe only; no claim of a PipeWire decoder |
| Accessibility | [Rust AT-SPI](https://docs.rs/atspi/latest/atspi/), [GNOME AT-SPI interfaces](https://gitlab.gnome.org/GNOME/at-spi2-core/-/tree/main/xml) | Normalize native AT-SPI D-Bus objects behind backend traits; no crate-type leakage |
| EIS | [reis Rust API](https://docs.rs/reis/latest/reis/), [libei](https://gitlab.freedesktop.org/libinput/libei) | `reis` sender transport implemented and exercised against a real EIS peer fixture; live compositor-granted portal transport remains pending |
| GNOME | [GJS extension guide](https://gjs.guide/extensions/), [Mutter API](https://mutter.gnome.org/meta/) | Narrow first-party extension, no arbitrary eval; candidate versions require live tests |
| KDE | [KWin scripting API](https://develop.kde.org/docs/plasma/kwin/api/) | Narrow script/mailbox rather than assume a generic Wayland window-control API |
| Sway | [Sway IPC manual source](https://github.com/swaywm/sway/blob/master/sway/sway-ipc.7.scd) | Typed native Unix framing, no shell command interpolation |
| Hyprland | [IPC documentation](https://wiki.hypr.land/IPC/) | Native local socket adapter; exact deployed version still must be tested |
| Filesystem confinement | [Linux openat2 manual](https://man7.org/linux/man-pages/man2/openat2.2.html) | Directory-FD-relative kernel resolution, no weak fallback |
| Landlock | [landlock Rust API](https://docs.rs/landlock/latest/landlock/) | Current docs showed 0.4.7; required ABI3 filesystem boundary in a single-threaded helper |
| Browser | [Chrome DevTools Protocol](https://chromedevtools.github.io/devtools-protocol/) | Broker-created profile, DOM/Input rather than exposed Runtime.evaluate |
| Blender | [Blender API reference](https://docs.blender.org/api/current/) | Typed bpy command allowlist; direct fetch was not reliable and no live Blender validation occurred |

## Existing projects and naming

The reviewed [agent-sh/computer-use-linux](https://github.com/agent-sh/computer-use-linux)
project already combines Linux desktop control with AT-SPI, GNOME and Wayland-related
routes. Semantic-first Linux automation is not claimed as a new invention. A second
comparison point was [linux-desktop-mcp](https://github.com/BeckhamLabsLLC/linux-desktop-mcp).
Semwright's intended scope is the integrated capability broker, shared typed registry,
recipes, app-native adapters, process SDK and auditable local execution—not a claim that
individual mechanisms are unprecedented or absent elsewhere.

Exact-name web searches for “semwright” with GitHub/crates/npm/Linux package qualifiers
did not establish a confirmed collision, but the returned search results were not a
reliable namespace audit. **The working name is provisional.** No crate/npm/GitHub name
was reserved, no trademark clearance was performed and no public repository URL is invented.
Node is used for tests/GJS source only; no npm runtime package is published.

Most Cargo dependencies use bounded major/minor compatibility ranges, while key researched
SDK/bus/sandbox dependencies are exact-pinned. This is not reproducibility without Cargo.lock.
Transitive security/license review and immutable CI dependency pinning remain blocked.
See [ADR 0001](adr/0001-delivered-scope.md) for scope reconciliation.
