# Product Vision

## Working description

A local-first, open-source **agentic computer interface for Linux**: a capability broker, CLI and MCP server that turn a Linux desktop into a typed, semantic command surface.

Canonical command name: `semwright`.

The project and canonical CLI name are now fixed as `Semwright` / `semwright`; future companion tools should preserve that namespace.

## Problem

Most “computer use” systems treat the desktop as pixels. That is universal but expensive, ambiguous and fragile.

A Linux desktop already exposes much richer interfaces:

- D-Bus services;
- AT-SPI accessibility trees;
- XDG Desktop Portals;
- PipeWire;
- libei/EIS;
- compositor/window-manager APIs;
- application-specific APIs;
- process/filesystem/system-service interfaces.

The project should expose these as a coherent command system so an agent can ask for **intent** instead of simulating a person.

## Product thesis

The agent should decide **what** to do. The system should decide the safest and most deterministic **how**.

Example:

```text
Agent intent:
"press the Export button in GIMP"

Preferred execution:
AT-SPI -> exact semantic element -> Action::Press

Not preferred:
screenshot -> OCR/vision -> estimate x/y -> inject click
```

For a richer application:

```text
Agent intent:
"create a cube in Blender and render it"

Preferred execution:
Blender adapter -> typed Blender command(s)

Fallback:
AT-SPI menu traversal

Last fallback:
mouse/keyboard/vision
```

## Core differentiators

### 1. Semantic-first, not screenshot-first

Every operation has an explicit backend preference ladder. The system records which backend executed each action.

### 2. Capability broker

The model does not need an unrestricted shell or unrestricted filesystem. It receives only the capabilities a policy profile allows.

### 3. Deterministic ambiguity handling

If two destructive targets match, the system fails with an `AmbiguousTarget` result. It does not “guess” silently.

### 4. One command registry, many front ends

CLI, MCP, recipes and plugins all use the same typed command registry and result types.

### 5. Extensible command packs

Application-specific integrations can add commands such as:

```text
blender.scene.inspect
blender.object.create
browser.tab.list
browser.dom.click
libreoffice.sheet.set_range
```

without changing the core broker.

### 6. Progressive compilation of workflows

A successful multi-step workflow can be converted into a reusable recipe or plugin command. Repeated agent improvisation becomes deterministic infrastructure.

### 7. Security is architectural

The preferred deployment does not hand arbitrary shell access to the model. Permissions are enforced in the broker and, where possible, by the OS.

### 8. Local-first

No telemetry by default. No cloud account required. Network access is not required for normal desktop control.

### 9. Explainability and auditability

Every action produces structured provenance:

```json
{
  "command": "ui.invoke",
  "target": "ui:42",
  "backend": "atspi",
  "policy_decision": "allow",
  "started_at": "...",
  "duration_ms": 18,
  "result": "ok"
}
```

Sensitive fields must be redacted.

## Intended users

- AI coding/automation agents.
- Power users automating Linux desktops.
- Tool builders adding desktop capabilities to agents.
- Researchers evaluating semantic vs visual computer use.
- Developers who need reliable automation across native Linux apps.
- Teams wanting a least-privilege alternative to giving an LLM a full shell.

## Product surfaces

### CLI

Fast, scriptable, JSON-capable:

```bash
semwright doctor
semwright window list --json
semwright ui snapshot --app org.gnome.Nautilus
semwright ui find --role button --name Save
semwright ui invoke ui:17
semwright recipe run export-assets
```

### MCP

Official Rust MCP SDK. A small discovery surface should avoid flooding model context with hundreds of tools.

### Daemon

User-level broker over a local Unix-domain socket. Owns capability state, policy, backend selection, sessions and audit.

### Inspector/TUI

A terminal UI for inspecting:
- detected environment;
- current windows;
- AT-SPI tree;
- element refs;
- enabled permissions;
- backends;
- audit events.

It should be useful for debugging even without an LLM.

### Plugin/adapter SDK

Out-of-process adapters by default. Plugins declare commands and required capabilities through manifests.

## Non-goals for Linux v1

- Building or shipping an LLM.
- Remote desktop over the internet.
- Keylogging.
- Credential harvesting.
- Silent bypass of portal consent.
- Running as root or setuid by default.
- Replacing the desktop environment.
- Making vision the primary automation strategy.
- Pretending one backend works identically on every compositor.
- Cross-platform Windows/macOS support in the first release.
- Exposing arbitrary code execution as the default agent interface.

## “Final” means v1.0-grade, not infinite scope

The requested result is not an MVP. It should be a coherent, releasable v1.0-grade repository with:
- core architecture implemented;
- multiple real Linux backends;
- policy and audit system;
- CLI + MCP;
- plugin/recipe framework;
- first-party integrations/examples;
- test harnesses;
- docs;
- packaging;
- CI;
- security policy;
- release automation.

It does **not** mean claiming universal GUI automation where Linux/Wayland intentionally requires user consent or desktop-specific integration.
