# Architecture of the delivered source

The public protocol is independent of D-Bus, X11, GJS, Blender and MCP types. The workspace
merges the blueprint's many small backend crates into `backends` and its broker/audit into
`core` to keep ownership and dependency direction visible without dozens of empty crates.

```text
computerctl / semwright-mcp / semwright-inspect
                │ bounded versioned Unix socket; session ticket
                ▼
semwrightd → Broker → Registry + Policy + RefStore + Audit
                │ permission decision; no widening fallback
                ├─ application adapters: Blender / private Chromium
                ├─ compositor routes: GNOME / KWin / Sway / Hyprland / X11
                ├─ semantic UI: AT-SPI
                ├─ consented portal / explicit clipboard helper
                ├─ filesystem / narrow system APIs
                └─ validated recipe step / sandboxed plugin
```

`types` owns errors, normalized nodes, selectors and references. `protocol` owns wire
frames and same-UID Unix clients. `registry` loads command schemas once. `policy` is mostly
pure and owns filesystem grants; kernel-specific FD confinement is isolated in its own
module. `backend-api` has no frontend dependencies. `recipes` depends on an abstract
executor, and every production step re-enters the broker. `plugin-sdk` is process-oriented;
`plugin-host` verifies and stages a binary before launching it inside isolation.

The broker resolves a session reference, evaluates capabilities/risk/scope, chooses a
backend, obtains the execution gate, requests human approval when necessary, re-resolves
and validates the target, and dispatches with a cancellation token and deadline. Reads
share a gate; mutations and operator approval are exclusive. Every admitted command has
start/finish metadata, including failures. A missing or unwritable audit sink fails closed.

Backend selection uses the descriptor candidate order. A reference cannot migrate to
another backend. A backend's execution error ends the operation: no implicit retry or
click fallback is allowed. There is not a universal semantic-equivalence engine that
maps any desktop control to an arbitrary app-native command. Application commands are
explicitly discovered and used when they express the desired intent more directly.

The reference store bounds memory and session lifetime. Backend markers are converted to
opaque refs only in broker output. A stale UI generation fails instead of silently
refreshing the target. Native backend identity quality still matters; see the documented
X11 limitation. A fresh capability probe may be cached for five seconds. The metadata
event bus has a finite replay window and reports cursor loss rather than pretending no
event was lost.

The daemon owns composition, not backend logic. No default TCP listener, root service,
or automatic elevated helper is created. See [protocol](protocol.md),
[permissions](permissions.md), and [the implementation deviations](adr/0001-delivered-scope.md).
All Rust behavior described here remains uncompiled in this handoff.
