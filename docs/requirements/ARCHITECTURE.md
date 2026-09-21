# Architecture

## Design rules

1. **Semantic action before visual action.**
2. **Typed command before arbitrary shell.**
3. **Policy checked before backend selection and again before side effect.**
4. **No silent fallback that widens permissions.**
5. **Backend capability discovery is runtime, not hard-coded assumptions.**
6. **Every side effect returns provenance.**
7. **Plugins are isolated processes by default.**
8. **CLI and MCP never implement desktop logic independently.**
9. **Live GUI limitations must be represented as capability states, not hidden errors.**
10. **Linux distribution/compositor fragmentation is an explicit architectural concern.**

## High-level layout

```text
                ┌────────────────────────────┐
                │ Clients                    │
                │ CLI / MCP / TUI / tests    │
                └──────────────┬─────────────┘
                               │ typed RPC
                               ▼
                ┌────────────────────────────┐
                │ User daemon / Broker       │
                │ session + registry         │
                └──────────────┬─────────────┘
                               │
             ┌─────────────────┼──────────────────┐
             ▼                 ▼                  ▼
       Policy engine      Command registry      Audit
             │                 │                  │
             └─────────────────┼──────────────────┘
                               ▼
                     Execution planner
                               │
       ┌───────────────┬───────┼────────┬────────────────┐
       ▼               ▼       ▼        ▼                ▼
 App adapters        D-Bus   AT-SPI   Window mgr       Input
 Blender/Browser      APIs    tree     compositor       portals/libei
       │               │       │        │                │
       └───────────────┴───────┼────────┴────────────────┘
                               ▼
                         optional vision
                         last-resort only
```

## Recommended language/runtime

### Rust for the core

Reasons:
- one fast distributable binary family;
- strong types for command schemas and policy;
- good async ecosystem;
- official MCP Rust SDK exists;
- zbus/ashpd ecosystem for D-Bus/portals;
- X11/Wayland crates exist;
- good CLI/testing/fuzzing ecosystem;
- easier to ship user-level daemon and static-ish release artifacts.

Use a Rust workspace.

### Python/JavaScript only where the host application requires it

Examples:
- Blender adapter add-on: Python inside Blender.
- GNOME Shell bridge: JavaScript/GJS.
- KWin script: JavaScript/QML if required by KWin.
- LibreOffice adapter may use Python/UNO if that is the most reliable route.

These must communicate through a narrow versioned protocol, not share process memory with the broker.

## Core components

### 1. Environment detector

Produces a normalized environment report:

```json
{
  "session_type": "wayland",
  "desktop": "gnome",
  "compositor": "mutter",
  "display": "...",
  "atspi": {"available": true},
  "portal": {
    "remote_desktop_version": 2,
    "screencast_version": 6
  },
  "input_backends": [
    {"name": "portal_eis", "state": "available"},
    {"name": "uinput", "state": "not_configured"}
  ]
}
```

The `doctor` command is built from this component.

### 2. Capability model

Capabilities are granular:

```text
desktop.observe
window.observe
window.manage
ui.observe
ui.invoke
input.keyboard
input.pointer
screen.capture
clipboard.read
clipboard.write
process.observe
process.manage
filesystem.read:<scope>
filesystem.write:<scope>
shell.exec
plugin:<name>:<command>
```

Capabilities can have scoped arguments such as:
- app ID;
- directory roots;
- executable allowlists;
- hostname/network policy;
- destructive confirmation requirement.

### 3. Command registry

Every command declares:
- stable command name;
- semantic version;
- input JSON Schema;
- output JSON Schema;
- capabilities required;
- risk class;
- idempotency classification;
- timeout;
- whether it can require interactive consent;
- whether it supports dry-run;
- possible backends.

Example metadata:

```json
{
  "name": "ui.invoke",
  "version": "1.0",
  "risk": "mutating",
  "requires": ["ui.invoke"],
  "idempotency": "unknown",
  "dry_run": true
}
```

### 4. Policy engine

Input:
- principal/client;
- command;
- normalized args;
- requested backend;
- environment;
- session permissions.

Output:

```text
ALLOW
DENY(reason)
REQUIRE_CONFIRMATION(prompt, scope)
ALLOW_WITH_REDACTIONS(...)
```

Policy must not be implemented only as an MCP annotation.

### 5. Execution planner

The planner chooses among deterministic backends according to a declared ladder.

For `ui.invoke`:

```text
1. application adapter, if it exposes equivalent command
2. AT-SPI Action interface
3. AT-SPI component + portal/libei pointer action
4. explicit input backend if policy allows
5. visual fallback only if enabled
```

It must return the selected backend.

No fallback may silently expand capability scope.

### 6. Reference store

The system returns short-lived references instead of forcing the model to repeat low-level identifiers:

```text
app:3
win:7
ui:42
screen:1
plugin:blender
```

Each ref carries:
- session id;
- backend identity;
- object identity;
- revision/generation;
- TTL where appropriate.

Stale refs return `StaleReference`, never target a newly reused object by accident.

### 7. Normalized accessibility model

Normalize AT-SPI into a stable project model.

Element fields should include:

```text
ref
role
name
description
states[]
actions[]
text_summary?
value?
bounds?
children_count
parent_ref?
relations?
application_ref
window_ref
```

Support compact snapshots and full snapshots.

For LLM use, default to compact snapshots with budgets:
- max depth;
- max nodes;
- include only actionable nodes;
- include changed nodes since revision.

### 8. Selector engine

Deterministic selectors:

```text
app
window
role
name exact
name regex
description
state
action
ancestor
descendant
nth
ref
```

Optional fuzzy search may exist for discovery but:
- returns ranked candidates;
- never auto-selects a mutating target above an ambiguity threshold;
- exact selector is required for destructive operations.

### 9. Event bus

Events:
- app launched/exited;
- window created/closed/focused;
- accessibility subtree changed;
- permission/session changed;
- plugin connected/disconnected;
- recipe step;
- command start/end/failure.

Used by CLI watch mode, TUI, tests and MCP subscriptions where supported.

### 10. Audit recorder

Structured append-only local logs, configurable retention.

Do not log:
- typed secrets;
- complete clipboard by default;
- complete screenshots;
- arbitrary file contents;
- auth headers.

Support audit levels:
`off`, `metadata`, `debug-redacted`.

## IPC design

Preferred:
- user daemon at `$XDG_RUNTIME_DIR/<project>/broker.sock`;
- Unix-domain socket;
- peer credential validation;
- length-prefixed JSON-RPC-like frames or another simple inspectable framed protocol;
- explicit protocol version negotiation;
- request IDs and cancellation;
- streaming/event channel support.

Avoid TCP for local default.

Large binary payloads:
- do not base64 multi-megabyte screenshots into every RPC response;
- use a temporary artifact handle/path inside a broker-owned runtime directory or FD passing if implemented safely;
- enforce expiry and permissions `0600`.

## Daemon lifecycle

- run as the logged-in user;
- systemd user service supported;
- CLI may offer safe autostart;
- never require root for core functionality;
- optional low-level input helpers must be explicit and separately configured.

## Error taxonomy

Stable machine-readable codes:

```text
Unsupported
Unavailable
PermissionDenied
ConsentRequired
PolicyDenied
NotFound
AmbiguousTarget
StaleReference
BackendFailed
Timeout
InvalidArgument
PluginProtocolError
SandboxDenied
Conflict
Cancelled
Internal
```

Human-readable messages are additional, not the contract.

## Idempotency and retries

Commands classify as:
- read-only;
- idempotent mutation;
- non-idempotent mutation;
- destructive.

The broker must not automatically retry non-idempotent/destructive mutations after uncertain failure.

Recipes can specify retry only on explicit safe error classes.

## Transactions

Do not pretend arbitrary desktop actions are ACID transactions.

Instead provide:
- preconditions;
- assertions;
- checkpoints;
- optional compensating actions;
- explicit `partial_success` state.

## Observability

Use structured tracing.
Support:
- `--log-format pretty|json`;
- trace IDs;
- command IDs;
- backend timing;
- diagnostics bundle with secrets redacted.

## Performance targets

These are engineering targets, not hard guarantees:

- CLI `doctor` warm response: sub-100 ms after daemon connection where probes are cached.
- simple registry lookup: single-digit ms.
- AT-SPI snapshot of a normal window: target sub-250 ms.
- exact `ui.invoke` after ref exists: target sub-100 ms excluding app latency.
- avoid screenshots unless requested/fallback is selected.
- no polling loops at high frequency when events are available.

## Compatibility philosophy

The system should report:

```text
SUPPORTED
SUPPORTED_WITH_CONSENT
SUPPORTED_WITH_HELPER
EXPERIMENTAL
UNAVAILABLE
```

per feature/backend.

Never collapse all Linux desktops into a single “works on Wayland” claim.
