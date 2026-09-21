# Commands, CLI, MCP, Recipes and Plugin SDK

## Command naming

Namespaces use lowercase dotted names:

```text
doctor
capabilities.list
app.list
app.launch
app.close
window.list
window.focus
window.move
window.resize
window.close
ui.snapshot
ui.find
ui.invoke
ui.set_text
ui.set_value
ui.toggle
ui.select
input.key
input.type
pointer.move
pointer.click
pointer.scroll
screen.capture
clipboard.read
clipboard.write
process.list
process.signal
filesystem.read
filesystem.write
recipe.list
recipe.run
plugin.list
plugin.describe
```

App adapters add namespaces:

```text
blender.object.list
blender.object.create
blender.render
browser.tab.list
browser.dom.query
browser.dom.click
```

## CLI UX

Human mode:

```bash
computerctl doctor
computerctl window list
computerctl ui snapshot --window win:3 --actionable
computerctl ui find --window win:3 --role button --name Save
computerctl ui invoke ui:19
```

Machine mode:

```bash
computerctl --json ui find ...
computerctl --json recipe run ...
```

Rules:
- stdout contains result;
- stderr contains diagnostics/logs;
- deterministic exit codes;
- no ANSI in `--json`;
- shell completion for bash/zsh/fish;
- man pages generated;
- `--help` is excellent.

## Command result envelope

Example:

```json
{
  "ok": true,
  "request_id": "req_...",
  "command": "ui.invoke",
  "data": {
    "invoked": true
  },
  "execution": {
    "backend": "atspi",
    "duration_ms": 24,
    "fallbacks_attempted": []
  },
  "warnings": []
}
```

Error:

```json
{
  "ok": false,
  "error": {
    "code": "AmbiguousTarget",
    "message": "3 buttons named Save matched",
    "candidates": ["ui:12", "ui:20", "ui:31"]
  }
}
```

## MCP surface

Use the official Rust MCP SDK.

Do not expose 150 tools to every client by default.

Recommended model:

### Always-visible tools

- `computer_doctor`
- `computer_capabilities`
- `computer_search_commands`
- `computer_describe_command`
- `computer_execute`
- `computer_snapshot`
- `computer_find`
- `computer_audit_tail` (metadata/redacted)

### Dynamic tools

If the negotiated MCP client supports dynamic tool list changes, commands can be enabled as individual tools for a session.

Example flow:

```text
search_commands("blender material")
-> blender.material.list
-> blender.material.create
-> blender.material.assign

enable those tools
```

If dynamic registration is not supported, `computer_execute` remains the universal typed gateway.

The MCP layer must:
- preserve input/output schemas;
- include risk annotations/metadata where applicable;
- return structured content;
- never bypass broker policy;
- support cancellation;
- support long-running tasks when useful and supported by the negotiated protocol.

## Context efficiency

Accessibility trees can be huge. Build model-facing compaction:

```text
ui.snapshot(
  root=win:3,
  mode="actionable",
  max_depth=5,
  max_nodes=200,
  changed_since="rev:123"
)
```

Optional text representation:

```text
[ui:11] button "Save" enabled action=press
[ui:12] button "Cancel" enabled action=press
[ui:13] entry "Filename" focused editable
```

Always preserve a structured representation for machine clients.

## Selector grammar

CLI examples:

```bash
computerctl ui find \
  --app org.gimp.GIMP \
  --role button \
  --name-exact Export

computerctl ui find \
  --window win:4 \
  --role menu-item \
  --name-regex '^Save( As…)?$'
```

Programmatic selector:

```json
{
  "within": "win:4",
  "role": "button",
  "name": {"op": "exact", "value": "Save"},
  "states": ["enabled", "visible"]
}
```

Fuzzy mode is discovery only:

```json
{
  "query": "save document button",
  "mode": "ranked_candidates"
}
```

It must not automatically invoke a mutation.

## Recipes

Recipes are declarative workflows, not shell scripts disguised as YAML.

Example:

```yaml
apiVersion: project/v1
kind: Recipe
metadata:
  name: save-active-document
inputs:
  path:
    type: path
    required: true
steps:
  - id: find_save_as
    command: ui.find
    args:
      within: "${context.active_window}"
      role: menu-item
      name:
        op: exact
        value: "Save As…"
    expect:
      exactly: 1

  - id: invoke
    command: ui.invoke
    args:
      ref: "${steps.find_save_as.single.ref}"

  - id: filename
    command: ui.set_text
    args:
      selector:
        role: entry
        focused: true
      text: "${inputs.path}"
```

Recipe engine requirements:
- typed inputs;
- schema validation;
- timeouts;
- assertions;
- explicit retries;
- `when` conditions;
- no arbitrary shell interpolation by default;
- secrets type that redacts values;
- dry-run plan;
- step-level audit;
- reusable output values;
- cancellation.

## “Compile a workflow into a command”

Provide tooling:

```bash
computerctl recipe scaffold my-workflow
computerctl plugin scaffold my-plugin
computerctl recipe validate recipe.yaml
computerctl recipe test recipe.yaml --backend fake
```

A future agent can turn an observed successful sequence into a recipe, but the project must never silently install generated code.

## Plugin architecture

### Principle

Out-of-process by default.

Why:
- crash isolation;
- language independence;
- sandboxing;
- version negotiation;
- no unsafe ABI coupling.

### Manifest

Example:

```toml
manifest_version = 1
name = "blender"
version = "1.0.0"
protocol = "1"
executable = "computerctl-plugin-blender"

[permissions]
network = "none"
filesystem_read = ["$XDG_RUNTIME_DIR/project/**"]
filesystem_write = ["$XDG_RUNTIME_DIR/project/**"]

[[commands]]
name = "blender.object.list"
risk = "read_only"
```

### Plugin protocol

Local stdio or broker-created Unix socket.

Handshake:
- protocol version;
- plugin identity/version;
- command schemas;
- health;
- requested capabilities.

Broker validates manifest against runtime handshake.

### Sandbox

Default plugin launch:
- scrubbed environment;
- no inherited secrets;
- controlled current directory;
- Landlock when available;
- optional bubblewrap;
- `no_new_privs`;
- resource limits;
- network disabled unless explicitly requested;
- process timeout/watchdog.

### Plugin signing/trust

Do not invent a centralized marketplace for v1.

Do support:
- local trust database;
- SHA-256 digest shown at install;
- provenance fields;
- explicit confirmation when a plugin requests dangerous capabilities.

## First-party plugin quality

First-party adapters must have:
- schema tests;
- fake-host tests;
- protocol conformance tests;
- version handshake;
- health/doctor report;
- installation docs;
- uninstall docs.

## Stable API

Maintain:
- command protocol version;
- plugin protocol version;
- recipe schema version;
- MCP mapping version if needed.

Use SemVer for the project and explicit migration docs.
