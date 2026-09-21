# Backends and Compatibility Strategy

## Backend preference ladder

For every action, prefer the highest semantic layer that can satisfy it.

### Tier A — application-native

Examples:
- Blender Python API through a controlled add-on/bridge.
- Chromium DevTools Protocol for browser DOM actions.
- LibreOffice UNO.
- application D-Bus service.

### Tier B — desktop/service-native

Examples:
- NetworkManager D-Bus.
- PipeWire/WirePlumber APIs.
- systemd user services.
- desktop notifications.
- compositor/window-manager APIs.

### Tier C — accessibility

AT-SPI2 for:
- application discovery;
- semantic widgets;
- roles;
- names;
- states;
- actions;
- text;
- values;
- component geometry.

### Tier D — approved remote-input path

XDG RemoteDesktop portal + EIS/libei where available.

### Tier E — low-level input

Optional:
- uinput helper;
- ydotool-style integration;
- X11 XTEST for X11.

Must be explicit and policy-gated.

### Tier F — visual

ScreenCast/Screenshot portal + optional vision adapter.

Last resort.

---

## Wayland strategy

Wayland intentionally prevents arbitrary cross-client observation/input. Do not fight the security model.

### Remote input

Use XDG Desktop Portal `RemoteDesktop` when the desktop provides it.

Current baseline verified on 2026-09-21:
- the portal exposes keyboard, pointer and touchscreen device selection;
- starting a session typically causes user-facing consent;
- the session can integrate ScreenCast and Clipboard;
- persistent/restore tokens may be available depending on portal/backend.

Prefer `ConnectToEIS`/libei through a maintained Rust route when supported.

If a desktop lacks a usable EIS route, expose the limitation and optionally support an explicit uinput helper configured by the user.

### Screen content

Use XDG ScreenCast portal and PipeWire.

Requirements:
- handle portal versions dynamically;
- preserve logical vs pixel coordinate distinctions;
- account for scaling;
- use mapping identifiers when available;
- do not assume a PipeWire node ID is globally permanent;
- consent behavior must be surfaced to the caller.

### GNOME

Likely combination:
- AT-SPI for semantic UI;
- RemoteDesktop portal for approved input;
- optional first-party GNOME Shell extension for precise window metadata/control not otherwise available in a stable generic interface.

The extension must:
- expose only a narrow D-Bus interface;
- validate caller/session where practical;
- never expose eval/arbitrary JS;
- have its own version handshake;
- be optional;
- degrade gracefully when absent.

### KDE Plasma / KWin

Use:
- AT-SPI;
- RemoteDesktop portal;
- first-party KWin script/bridge for richer window management where needed.

KWin supports scripting for programmatic window manipulation. Keep the bridge narrow and versioned.

### wlroots family

For Sway/Hyprland and similar:
- use compositor-native IPC when available and stable;
- Sway: `swaymsg`/IPC adapter or direct IPC protocol implementation;
- Hyprland: socket/`hyprctl` adapter where appropriate;
- keep these in separate backend crates.

Do not bake shell command parsing into core domain logic.

### COSMIC and future desktops

Backend trait plus runtime detection must make new compositor support additive.

---

## X11 strategy

X11 is less restrictive but still deserves semantic-first behavior.

Use:
- AT-SPI first;
- EWMH/X11 window management through a Rust X11 client;
- XTEST or equivalent input only when needed;
- screenshots only as fallback.

Do not make X11-era tools like `xdotool` the architectural center.

A compatibility adapter can invoke an external tool if necessary, but internal typed backends are preferred.

---

## AT-SPI strategy

### Why it matters

AT-SPI is the semantic bridge for GTK, Qt, Electron and other apps that expose accessibility.

### Requirements

Implement:
- application enumeration;
- tree traversal with budgets;
- role/name/state/action normalization;
- Action invocation;
- text read/set where interface permits;
- value interface;
- component geometry;
- event subscriptions;
- robust handling of disappearing objects and D-Bus errors.

### Performance

Avoid full recursive tree scans by default.

Use:
- cached node metadata;
- generation IDs;
- event-driven invalidation;
- depth/node limits;
- actionable-only snapshots;
- delta snapshots.

### Accessibility gaps

Some apps/widgets do not expose enough semantics.

The result must identify this:

```json
{
  "semantic_coverage": "partial",
  "reason": "application did not expose actionable child controls"
}
```

Then the planner may fall back, if policy allows.

---

## D-Bus/system adapters

First-party system commands should demonstrate that GUI automation is not necessary for many tasks.

Candidate namespaces:
- `audio.*`
- `network.*`
- `power.*`
- `notifications.*`
- `systemd.user.*`

Do not implement unsafe privileged system administration in v1 by default.

---

## First-party application adapters

At least two real adapters should ship to prove the architecture.

### Blender adapter

Strong candidate because Blender has a rich Python API.

Architecture:
- Blender add-on installed separately;
- local IPC endpoint under `$XDG_RUNTIME_DIR`;
- random session token or peer-authenticated Unix socket;
- allowlisted typed operations;
- no arbitrary `python.exec` in default agent surface.

Initial command families:
- scene inspect;
- object list/create/delete/transform;
- material list/create/assign;
- collection operations;
- render settings;
- render;
- save/open with scoped paths.

Advanced direct Python may exist only behind a separate high-risk developer capability.

### Chromium-family browser adapter

Use CDP only against:
- an instance launched by the project with an isolated profile; or
- an explicitly configured debug endpoint.

Do not silently attach to the user's normal profile.

Commands:
- tabs list/open/close/focus;
- DOM snapshot/query;
- click/fill;
- navigation;
- download tracking;
- screenshot;
- console/network summaries where requested.

Web UI automation belongs here rather than through desktop pixels.

### Optional third adapter

LibreOffice UNO or GIMP 3 plugin, whichever can be implemented and verified more reliably in the build environment.

---

## Capability probing

`computerctl doctor --json` must expose a matrix like:

```json
{
  "features": {
    "ui.inspect": {
      "status": "supported",
      "backend": "atspi"
    },
    "input.pointer": {
      "status": "supported_with_consent",
      "backend": "portal_eis"
    },
    "window.move": {
      "status": "supported_with_helper",
      "backend": "gnome_extension"
    }
  }
}
```

This is a core product feature, not a debug afterthought.

---

## Required manual validation matrix

The repository must document a validation matrix covering at least:

1. GNOME + Wayland
2. KDE Plasma + Wayland
3. Sway or another wlroots compositor
4. Hyprland
5. X11 desktop/session
6. x86_64
7. aarch64 build

The code must distinguish “CI compiled/tested” from “manually verified”.
