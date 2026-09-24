# Command reference

Generated from `schemas/commands.json`; do not edit by hand.

105 built-in descriptors. A descriptor is not proof of live backend support.
Run `semwright doctor` and consult `compatibility.md` and `../VERIFY.md`.

Every command accepts only its documented properties. Use `commands describe NAME`
for the authoritative input/output schema. Return schemas are intentionally broad
for many backends in this development handoff; strengthening them is a release gate.

| Command | Required capability | Risk | Timeout | Candidate backends |
|---|---|---|---:|---|
| `doctor` | `desktop.observe` | read_only | 10000 ms | core |
| `capabilities.list` | `desktop.observe` | read_only | 10000 ms | core |
| `commands.search` | `desktop.observe` | read_only | 10000 ms | core |
| `commands.describe` | `desktop.observe` | read_only | 10000 ms | core |
| `audit.tail` | `desktop.observe` | read_only | 10000 ms | core |
| `plugin.list` | `desktop.observe` | read_only | 10000 ms | core |
| `plugin.describe` | `desktop.observe` | read_only | 10000 ms | core |
| `recipe.list` | `desktop.observe` | read_only | 10000 ms | core |
| `recipe.validate` | `desktop.observe` | read_only | 10000 ms | core |
| `recipe.run` | `desktop.observe` | mutating | 300000 ms | core |
| `app.list` | `app.observe` | read_only | 10000 ms | atspi, macos |
| `app.launch` | `app.launch` | code_execution | 10000 ms | system |
| `app.close` | `app.close`, `window.manage` | destructive | 10000 ms | sway, hyprland, gnome, kwin, x11 |
| `window.list` | `window.observe` | read_only | 10000 ms | sway, hyprland, gnome, kwin, x11, macos |
| `window.focus` | `window.manage` | mutating_reversible | 10000 ms | sway, hyprland, gnome, kwin, x11, macos |
| `window.move` | `window.manage` | mutating_reversible | 10000 ms | sway, hyprland, gnome, kwin, x11, macos |
| `window.resize` | `window.manage` | mutating_reversible | 10000 ms | sway, hyprland, gnome, kwin, x11, macos |
| `window.close` | `window.manage` | destructive | 10000 ms | sway, hyprland, gnome, kwin, x11, macos |
| `ui.snapshot` | `ui.observe` | read_only | 10000 ms | atspi, macos |
| `ui.find` | `ui.observe` | read_only | 10000 ms | core |
| `ui.invoke` | `ui.invoke` | mutating | 10000 ms | atspi, macos |
| `ui.set_text` | `ui.invoke` | mutating | 10000 ms | atspi, macos |
| `ui.read_text` | `ui.text.read` | secret_access | 10000 ms | atspi, macos |
| `ui.set_value` | `ui.invoke` | mutating_reversible | 10000 ms | atspi, macos |
| `ui.get_value` | `ui.observe` | read_only | 10000 ms | atspi, macos |
| `ui.toggle` | `ui.invoke` | mutating | 10000 ms | atspi, macos |
| `ui.select` | `ui.invoke` | mutating | 10000 ms | atspi |
| `ui.expand` | `ui.invoke` | mutating | 10000 ms | atspi, macos |
| `portal.start` | `input.keyboard`, `input.pointer` | privilege_sensitive | 120000 ms | portal |
| `portal.stop` | `desktop.observe` | mutating_reversible | 10000 ms | portal |
| `portal.status` | `desktop.observe` | read_only | 10000 ms | portal |
| `portal.restore.clear` | `desktop.observe` | mutating | 10000 ms | portal |
| `input.key` | `input.keyboard` | mutating | 10000 ms | portal, x11, macos |
| `input.type` | `input.keyboard` | mutating | 10000 ms | portal, macos |
| `pointer.move` | `input.pointer` | mutating | 10000 ms | portal, x11, macos |
| `pointer.click` | `input.pointer` | mutating | 10000 ms | portal, x11, macos |
| `pointer.scroll` | `input.pointer` | mutating | 10000 ms | portal, macos |
| `screen.capture` | `screen.capture` | secret_access | 120000 ms | portal, macos |
| `screen.stream_info` | `desktop.observe` | read_only | 10000 ms | portal |
| `screen.stream.start` | `screen.capture` | secret_access | 120000 ms | portal |
| `screen.stream.capture` | `screen.capture` | secret_access | 45000 ms | portal |
| `screen.stream.stop` | `screen.capture` | mutating_reversible | 10000 ms | portal |
| `clipboard.read` | `clipboard.read` | secret_access | 10000 ms | clipboard, portal, macos |
| `clipboard.write` | `clipboard.write` | mutating | 10000 ms | clipboard, portal, macos |
| `process.list` | `process.observe` | read_only | 10000 ms | system |
| `process.signal` | `process.manage` | destructive | 10000 ms | system |
| `filesystem.read` | `filesystem.read` | read_only | 10000 ms | filesystem |
| `filesystem.write` | `filesystem.write` | mutating_reversible | 10000 ms | filesystem |
| `artifact.handoff` | `filesystem.read:source_root`, `filesystem.write:destination_root` | mutating | 30000 ms | artifacts |
| `notifications.send` | `notifications.send` | mutating | 10000 ms | system |
| `network.status` | `desktop.observe` | read_only | 10000 ms | system |
| `systemd.user.status` | `desktop.observe` | read_only | 10000 ms | system |
| `blender.status` | `blender.observe` | read_only | 30000 ms | blender |
| `blender.scene.inspect` | `blender.observe` | read_only | 30000 ms | blender |
| `blender.object.list` | `blender.observe` | read_only | 30000 ms | blender |
| `blender.object.get` | `blender.observe` | read_only | 30000 ms | blender |
| `blender.object.create` | `blender.modify` | mutating | 30000 ms | blender |
| `blender.object.delete` | `blender.modify` | destructive | 30000 ms | blender |
| `blender.object.transform` | `blender.modify` | mutating_reversible | 30000 ms | blender |
| `blender.collection.list` | `blender.observe` | read_only | 30000 ms | blender |
| `blender.collection.create` | `blender.modify` | mutating | 30000 ms | blender |
| `blender.collection.link` | `blender.modify` | mutating_reversible | 30000 ms | blender |
| `blender.material.list` | `blender.observe` | read_only | 30000 ms | blender |
| `blender.material.create` | `blender.modify` | mutating | 30000 ms | blender |
| `blender.material.assign` | `blender.modify` | mutating_reversible | 30000 ms | blender |
| `blender.render.settings` | `blender.modify` | mutating_reversible | 30000 ms | blender |
| `blender.render` | `blender.modify` | mutating | 300000 ms | blender |
| `blender.file.save` | `blender.modify` | destructive | 30000 ms | blender |
| `blender.file.open` | `blender.modify` | destructive | 30000 ms | blender |
| `browser.status` | `browser.observe` | read_only | 60000 ms | chromium |
| `browser.launch` | `browser.modify` | code_execution | 60000 ms | chromium |
| `browser.tab.list` | `browser.observe` | read_only | 60000 ms | chromium |
| `browser.tab.open` | `browser.modify` | mutating | 60000 ms | chromium |
| `browser.tab.close` | `browser.modify` | destructive | 60000 ms | chromium |
| `browser.tab.focus` | `browser.modify` | mutating_reversible | 60000 ms | chromium |
| `browser.navigate` | `browser.modify` | mutating | 60000 ms | chromium |
| `browser.dom.snapshot` | `browser.observe` | read_only | 60000 ms | chromium |
| `browser.dom.query` | `browser.observe` | read_only | 60000 ms | chromium |
| `browser.dom.click` | `browser.modify` | mutating | 60000 ms | chromium |
| `browser.dom.fill` | `browser.modify` | mutating | 60000 ms | chromium |
| `browser.screenshot` | `screen.capture` | secret_access | 60000 ms | chromium |
| `browser.downloads.status` | `browser.observe` | read_only | 60000 ms | chromium |
| `browser.diagnostics` | `browser.observe` | read_only | 60000 ms | chromium |
| `plugin.install` | `plugin.manage` | code_execution | 30000 ms | core |
| `plugin.remove` | `plugin.manage` | destructive | 30000 ms | core |
| `plugin.doctor` | `desktop.observe` | read_only | 30000 ms | core |
| `capabilities.search` | `desktop.observe` | read_only | 10000 ms | core |
| `capabilities.describe` | `desktop.observe` | read_only | 10000 ms | core |
| `jobs.start` | `desktop.observe` | read_only | 10000 ms | core |
| `jobs.list` | `desktop.observe` | read_only | 10000 ms | core |
| `jobs.get` | `desktop.observe` | read_only | 10000 ms | core |
| `jobs.cancel` | `desktop.observe` | read_only | 10000 ms | core |
| `workflow.candidate.delete` | `workflow.manage` | destructive | 10000 ms | core |
| `workflow.candidate.get` | `workflow.record` | read_only | 10000 ms | core |
| `workflow.compile` | `workflow.record` | mutating_reversible | 30000 ms | core |
| `workflow.demote` | `workflow.manage` | mutating_reversible | 30000 ms | core |
| `workflow.promote` | `workflow.manage` | mutating_reversible | 30000 ms | core |
| `workflow.promotions.list` | `workflow.record` | read_only | 10000 ms | core |
| `workflow.record.start` | `workflow.record` | mutating_reversible | 10000 ms | core |
| `workflow.record.stop` | `workflow.record` | mutating_reversible | 10000 ms | core |
| `workflow.replay` | `workflow.record` | mutating | 300000 ms | core |
| `workflow.trace.delete` | `workflow.manage` | destructive | 10000 ms | core |
| `workflow.trace.get` | `workflow.record` | read_only | 10000 ms | core |
| `workflow.traces.list` | `workflow.record` | read_only | 10000 ms | core |
| `workflow.verify` | `workflow.record` | mutating_reversible | 30000 ms | core |

## `doctor`

Probe available backends, current policy, session and remediation instructions.

Idempotency: `read_only`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {},
  "required": [],
  "additionalProperties": false
}
```

## `capabilities.list`

Return capabilities granted to this session, not merely installed APIs.

Idempotency: `read_only`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {},
  "required": [],
  "additionalProperties": false
}
```

## `commands.search`

Search the shared typed command registry.

Idempotency: `read_only`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "query": {
      "type": "string",
      "maxLength": 256
    },
    "limit": {
      "type": "integer",
      "minimum": 1,
      "maximum": 100
    }
  },
  "required": [],
  "additionalProperties": false
}
```

## `commands.describe`

Describe input/output schemas, risk and backend candidates.

Idempotency: `read_only`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "name": {
      "type": "string",
      "maxLength": 128
    }
  },
  "required": [
    "name"
  ],
  "additionalProperties": false
}
```

## `audit.tail`

Return recent metadata-only audit records.

Idempotency: `read_only`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "limit": {
      "type": "integer",
      "minimum": 1,
      "maximum": 500
    }
  },
  "required": [],
  "additionalProperties": false
}
```

## `plugin.list`

List configured, digest-pinned plugins and sandbox status.

Idempotency: `read_only`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {},
  "required": [],
  "additionalProperties": false
}
```

## `plugin.describe`

Describe a configured plugin manifest.

Idempotency: `read_only`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "name": {
      "type": "string",
      "maxLength": 80
    }
  },
  "required": [
    "name"
  ],
  "additionalProperties": false
}
```

## `recipe.list`

List example recipes available in the repository.

Idempotency: `read_only`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {},
  "required": [],
  "additionalProperties": false
}
```

## `recipe.validate`

Validate a versioned declarative recipe without executing it.

Idempotency: `read_only`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "recipe": {
      "type": "object",
      "maxProperties": 20
    }
  },
  "required": [
    "recipe"
  ],
  "additionalProperties": false
}
```

## `recipe.run`

Execute a validated recipe; each step re-enters the broker.

Idempotency: `non_idempotent`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "recipe": {
      "type": "object",
      "maxProperties": 20
    },
    "inputs": {
      "type": "object",
      "maxProperties": 64
    }
  },
  "required": [
    "recipe"
  ],
  "additionalProperties": false
}
```

## `app.list`

Enumerate accessible applications; incomplete accessibility is reported.

Idempotency: `read_only`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {},
  "required": [],
  "additionalProperties": false
}
```

## `app.launch`

Launch only an administrator-configured application key, never arbitrary argv.

Idempotency: `non_idempotent`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "application": {
      "type": "string",
      "maxLength": 128
    }
  },
  "required": [
    "application"
  ],
  "additionalProperties": false
}
```

## `app.close`

Close a referenced application window with external confirmation.

Idempotency: `destructive`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "ref": {
      "type": "string",
      "maxLength": 80,
      "pattern": "^(ui|win|app|dom|tab|screen|process):[0-9a-f]{32}$"
    }
  },
  "required": [
    "ref"
  ],
  "additionalProperties": false
}
```

## `window.list`

List compositor-native windows with opaque session references.

Idempotency: `read_only`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {},
  "required": [],
  "additionalProperties": false
}
```

## `window.focus`

Focus the exact referenced window.

Idempotency: `idempotent`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "ref": {
      "type": "string",
      "maxLength": 80,
      "pattern": "^(ui|win|app|dom|tab|screen|process):[0-9a-f]{32}$"
    }
  },
  "required": [
    "ref"
  ],
  "additionalProperties": false
}
```

## `window.move`

Move a referenced window in logical compositor coordinates.

Idempotency: `idempotent`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "ref": {
      "type": "string",
      "maxLength": 80,
      "pattern": "^(ui|win|app|dom|tab|screen|process):[0-9a-f]{32}$"
    },
    "x": {
      "type": "integer",
      "minimum": -32768,
      "maximum": 32767
    },
    "y": {
      "type": "integer",
      "minimum": -32768,
      "maximum": 32767
    }
  },
  "required": [
    "ref",
    "x",
    "y"
  ],
  "additionalProperties": false
}
```

## `window.resize`

Resize a referenced window in logical compositor coordinates.

Idempotency: `idempotent`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "ref": {
      "type": "string",
      "maxLength": 80,
      "pattern": "^(ui|win|app|dom|tab|screen|process):[0-9a-f]{32}$"
    },
    "width": {
      "type": "integer",
      "minimum": 1,
      "maximum": 16384
    },
    "height": {
      "type": "integer",
      "minimum": 1,
      "maximum": 16384
    }
  },
  "required": [
    "ref",
    "width",
    "height"
  ],
  "additionalProperties": false
}
```

## `window.close`

Request application close; unsaved work may be lost.

Idempotency: `destructive`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "ref": {
      "type": "string",
      "maxLength": 80,
      "pattern": "^(ui|win|app|dom|tab|screen|process):[0-9a-f]{32}$"
    }
  },
  "required": [
    "ref"
  ],
  "additionalProperties": false
}
```

## `ui.snapshot`

Bounded semantic accessibility snapshot with conservative revision deltas. No screenshot or OCR.

Idempotency: `read_only`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "app": {
      "type": "string",
      "maxLength": 255
    },
    "max_nodes": {
      "type": "integer",
      "minimum": 1,
      "maximum": 2000
    },
    "max_depth": {
      "type": "integer",
      "minimum": 0,
      "maximum": 32
    },
    "actionable": {
      "type": "boolean"
    },
    "since_revision": {
      "type": "integer",
      "minimum": 0
    }
  },
  "required": [],
  "additionalProperties": false
}
```

## `ui.find`

Exact or ranked discovery. Ambiguous matches are returned, never auto-invoked.

Idempotency: `read_only`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "selector": {
      "type": "object",
      "properties": {
        "app": {
          "type": "string",
          "maxLength": 255
        },
        "role": {
          "type": "string",
          "maxLength": 80
        },
        "name": {
          "type": "object",
          "properties": {
            "op": {
              "type": "string",
              "enum": [
                "exact",
                "regex"
              ]
            },
            "value": {
              "type": "string",
              "maxLength": 512
            }
          },
          "required": [
            "op",
            "value"
          ],
          "additionalProperties": false
        },
        "states": {
          "type": "array",
          "items": {
            "type": "string",
            "maxLength": 80
          },
          "maxItems": 32
        },
        "action": {
          "type": "string",
          "maxLength": 80
        },
        "ancestor": {
          "type": "string",
          "maxLength": 80,
          "pattern": "^(ui|win|app|dom|tab|screen|process):[0-9a-f]{32}$"
        },
        "nth": {
          "type": "integer",
          "minimum": 0,
          "maximum": 1999
        },
        "query": {
          "type": "string",
          "maxLength": 256
        }
      },
      "required": [],
      "additionalProperties": false
    },
    "max_nodes": {
      "type": "integer",
      "minimum": 1,
      "maximum": 2000
    },
    "max_depth": {
      "type": "integer",
      "minimum": 0,
      "maximum": 32
    }
  },
  "required": [
    "selector"
  ],
  "additionalProperties": false
}
```

## `ui.invoke`

Invoke an advertised accessibility Action on an exact live reference.

Idempotency: `non_idempotent`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "ref": {
      "type": "string",
      "maxLength": 80,
      "pattern": "^(ui|win|app|dom|tab|screen|process):[0-9a-f]{32}$"
    },
    "action": {
      "type": "string",
      "maxLength": 80
    }
  },
  "required": [
    "ref"
  ],
  "additionalProperties": false
}
```

## `ui.set_text`

Set editable text through AT-SPI, not through keyboard emulation.

Idempotency: `non_idempotent`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "ref": {
      "type": "string",
      "maxLength": 80,
      "pattern": "^(ui|win|app|dom|tab|screen|process):[0-9a-f]{32}$"
    },
    "text": {
      "type": "string",
      "maxLength": 65536
    }
  },
  "required": [
    "ref",
    "text"
  ],
  "additionalProperties": false
}
```

## `ui.read_text`

Read non-password accessible text with explicit permission.

Idempotency: `non_idempotent`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "ref": {
      "type": "string",
      "maxLength": 80,
      "pattern": "^(ui|win|app|dom|tab|screen|process):[0-9a-f]{32}$"
    },
    "max_chars": {
      "type": "integer",
      "minimum": 1,
      "maximum": 65536
    }
  },
  "required": [
    "ref"
  ],
  "additionalProperties": false
}
```

## `ui.set_value`

Set an accessible numeric value within the host-reported range.

Idempotency: `idempotent`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "ref": {
      "type": "string",
      "maxLength": 80,
      "pattern": "^(ui|win|app|dom|tab|screen|process):[0-9a-f]{32}$"
    },
    "value": {
      "type": "number",
      "minimum": -1000000000.0,
      "maximum": 1000000000.0
    }
  },
  "required": [
    "ref",
    "value"
  ],
  "additionalProperties": false
}
```

## `ui.get_value`

Read the accessible numeric value and range.

Idempotency: `read_only`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "ref": {
      "type": "string",
      "maxLength": 80,
      "pattern": "^(ui|win|app|dom|tab|screen|process):[0-9a-f]{32}$"
    }
  },
  "required": [
    "ref"
  ],
  "additionalProperties": false
}
```

## `ui.toggle`

Invoke an explicitly advertised toggle action.

Idempotency: `non_idempotent`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "ref": {
      "type": "string",
      "maxLength": 80,
      "pattern": "^(ui|win|app|dom|tab|screen|process):[0-9a-f]{32}$"
    }
  },
  "required": [
    "ref"
  ],
  "additionalProperties": false
}
```

## `ui.select`

Select a child through the AT-SPI Selection interface.

Idempotency: `non_idempotent`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "ref": {
      "type": "string",
      "maxLength": 80,
      "pattern": "^(ui|win|app|dom|tab|screen|process):[0-9a-f]{32}$"
    },
    "index": {
      "type": "integer",
      "minimum": 0,
      "maximum": 2000
    }
  },
  "required": [
    "ref",
    "index"
  ],
  "additionalProperties": false
}
```

## `ui.expand`

Invoke an explicitly advertised expand action.

Idempotency: `non_idempotent`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "ref": {
      "type": "string",
      "maxLength": 80,
      "pattern": "^(ui|win|app|dom|tab|screen|process):[0-9a-f]{32}$"
    }
  },
  "required": [
    "ref"
  ],
  "additionalProperties": false
}
```

## `portal.start`

Request a RemoteDesktop session with native consent; persistence and portal clipboard access are explicit opt-ins.

Idempotency: `non_idempotent`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "keyboard": {
      "type": "boolean"
    },
    "pointer": {
      "type": "boolean"
    },
    "persist_mode": {
      "type": "integer",
      "minimum": 0,
      "maximum": 2,
      "description": "0 ephemeral, 1 process lifetime, 2 persist until desktop permission is revoked"
    },
    "clipboard": {
      "type": "boolean",
      "description": "Request Clipboard portal integration for this RemoteDesktop session"
    }
  },
  "required": [],
  "additionalProperties": false
}
```

## `portal.stop`

Revoke and close this broker RemoteDesktop session.

Idempotency: `idempotent`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {},
  "required": [],
  "additionalProperties": false
}
```

## `portal.status`

Inspect portal consent/session state, interface versions and restore-token availability without exposing token material.

Idempotency: `read_only`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {},
  "required": [],
  "additionalProperties": false
}
```

## `portal.restore.clear`

Forget process-local and durable RemoteDesktop restore tokens without claiming to revoke the desktop portal permission itself.

Idempotency: `idempotent`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {},
  "required": [],
  "additionalProperties": false
}
```

## `input.key`

Send a keysym through an explicitly enabled input backend with focus precondition.

Idempotency: `non_idempotent`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "keysym": {
      "type": "integer",
      "minimum": 0,
      "maximum": 4294967295
    },
    "ref": {
      "type": "string",
      "maxLength": 80,
      "pattern": "^(ui|win|app|dom|tab|screen|process):[0-9a-f]{32}$"
    }
  },
  "required": [
    "keysym",
    "ref"
  ],
  "additionalProperties": false
}
```

## `input.type`

Type Unicode using a consented portal or focused X11; semantic set_text is preferred.

Idempotency: `non_idempotent`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "text": {
      "type": "string",
      "maxLength": 4096
    },
    "ref": {
      "type": "string",
      "maxLength": 80,
      "pattern": "^(ui|win|app|dom|tab|screen|process):[0-9a-f]{32}$"
    }
  },
  "required": [
    "text",
    "ref"
  ],
  "additionalProperties": false
}
```

## `pointer.move`

Move pointer relative to its current location (logical delta), guarded by expected focus.

Idempotency: `non_idempotent`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "dx": {
      "type": "number",
      "minimum": -10000,
      "maximum": 10000
    },
    "dy": {
      "type": "number",
      "minimum": -10000,
      "maximum": 10000
    },
    "ref": {
      "type": "string",
      "maxLength": 80,
      "pattern": "^(ui|win|app|dom|tab|screen|process):[0-9a-f]{32}$"
    }
  },
  "required": [
    "dx",
    "dy",
    "ref"
  ],
  "additionalProperties": false
}
```

## `pointer.click`

Click the current pointer position; requires an explicit live window reference.

Idempotency: `non_idempotent`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "button": {
      "type": "string",
      "enum": [
        "left",
        "middle",
        "right"
      ]
    },
    "ref": {
      "type": "string",
      "maxLength": 80,
      "pattern": "^(ui|win|app|dom|tab|screen|process):[0-9a-f]{32}$"
    }
  },
  "required": [
    "button",
    "ref"
  ],
  "additionalProperties": false
}
```

## `pointer.scroll`

Scroll through the approved pointer backend; no raw coordinate fallback.

Idempotency: `non_idempotent`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "dx": {
      "type": "number",
      "minimum": -1000,
      "maximum": 1000
    },
    "dy": {
      "type": "number",
      "minimum": -1000,
      "maximum": 1000
    },
    "ref": {
      "type": "string",
      "maxLength": 80,
      "pattern": "^(ui|win|app|dom|tab|screen|process):[0-9a-f]{32}$"
    }
  },
  "required": [
    "dx",
    "dy",
    "ref"
  ],
  "additionalProperties": false
}
```

## `screen.capture`

Request an interactive screenshot and copy it to a private expiring artifact.

Idempotency: `non_idempotent`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {},
  "required": [],
  "additionalProperties": false
}
```

## `screen.stream_info`

Describe ScreenCast support and the owner session's active PipeWire streams.

Idempotency: `read_only`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {},
  "required": [],
  "additionalProperties": false
}
```

## `screen.stream.start`

Request a user-consented ScreenCast session for bounded PipeWire frame capture.

Idempotency: `non_idempotent`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "source": {
      "type": "string",
      "enum": [
        "any",
        "monitor",
        "window"
      ]
    },
    "multiple": {
      "type": "boolean"
    },
    "cursor": {
      "type": "string",
      "enum": [
        "hidden",
        "embedded"
      ]
    }
  },
  "required": [],
  "additionalProperties": false
}
```

## `screen.stream.capture`

Capture one bounded frame from an active owner ScreenCast stream into a private PNG artifact.

Idempotency: `non_idempotent`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "stream": {
      "type": "integer",
      "minimum": 0,
      "maximum": 15
    },
    "timeout_ms": {
      "type": "integer",
      "minimum": 100,
      "maximum": 30000
    }
  },
  "required": [],
  "additionalProperties": false
}
```

## `screen.stream.stop`

Revoke and close this broker session's active ScreenCast grant.

Idempotency: `idempotent`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {},
  "required": [],
  "additionalProperties": false
}
```

## `clipboard.read`

Read clipboard text only when explicitly granted; never audit its contents.

Idempotency: `non_idempotent`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "max_bytes": {
      "type": "integer",
      "minimum": 1,
      "maximum": 1048576
    }
  },
  "required": [],
  "additionalProperties": false
}
```

## `clipboard.write`

Set clipboard text using an explicit backend, without shell interpolation.

Idempotency: `non_idempotent`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "text": {
      "type": "string",
      "maxLength": 65536
    }
  },
  "required": [
    "text"
  ],
  "additionalProperties": false
}
```

## `process.list`

List current-user process metadata without command lines or environment variables.

Idempotency: `read_only`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {},
  "required": [],
  "additionalProperties": false
}
```

## `process.signal`

Signal only a process launched and retained by this broker.

Idempotency: `destructive`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "ref": {
      "type": "string",
      "maxLength": 80,
      "pattern": "^(ui|win|app|dom|tab|screen|process):[0-9a-f]{32}$"
    },
    "signal": {
      "type": "string",
      "enum": [
        "term",
        "int"
      ]
    }
  },
  "required": [
    "ref",
    "signal"
  ],
  "additionalProperties": false
}
```

## `filesystem.read`

Descriptor-relative, no-symlink read under an explicitly configured root.

Idempotency: `read_only`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "root": {
      "type": "string",
      "maxLength": 64,
      "pattern": "^[a-zA-Z0-9_-]+$"
    },
    "path": {
      "type": "string",
      "maxLength": 4096
    },
    "max_bytes": {
      "type": "integer",
      "minimum": 1,
      "maximum": 1048576
    }
  },
  "required": [
    "root",
    "path"
  ],
  "additionalProperties": false
}
```

## `filesystem.write`

Descriptor-relative, no-symlink write under an explicitly configured root.

Idempotency: `idempotent`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "root": {
      "type": "string",
      "maxLength": 64,
      "pattern": "^[a-zA-Z0-9_-]+$"
    },
    "path": {
      "type": "string",
      "maxLength": 4096
    },
    "text": {
      "type": "string",
      "maxLength": 1048576
    }
  },
  "required": [
    "root",
    "path",
    "text"
  ],
  "additionalProperties": false
}
```

## `artifact.handoff`

Copy a bounded binary artifact between two explicitly granted filesystem roots without exposing host absolute paths.

Idempotency: `idempotent`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "source_root": {
      "type": "string",
      "maxLength": 64,
      "pattern": "^[a-zA-Z0-9_-]+$"
    },
    "source_path": {
      "type": "string",
      "maxLength": 4096
    },
    "destination_root": {
      "type": "string",
      "maxLength": 64,
      "pattern": "^[a-zA-Z0-9_-]+$"
    },
    "destination_path": {
      "type": "string",
      "maxLength": 4096
    },
    "max_bytes": {
      "type": "integer",
      "minimum": 1,
      "maximum": 67108864
    },
    "expected_sha256": {
      "type": "string",
      "pattern": "^[0-9a-fA-F]{64}$"
    },
    "semantic_type": {
      "type": "string",
      "maxLength": 96,
      "pattern": "^[a-z0-9][a-z0-9.+-]*/[a-z0-9][a-z0-9.+-]*$"
    },
    "media_type": {
      "type": "string",
      "maxLength": 128,
      "pattern": "^[A-Za-z0-9][A-Za-z0-9.+-]*/[A-Za-z0-9][A-Za-z0-9.+-]*$"
    }
  },
  "required": [
    "source_root",
    "source_path",
    "destination_root",
    "destination_path"
  ],
  "additionalProperties": false
}
```

## `notifications.send`

Send a desktop notification through the Notifications D-Bus interface.

Idempotency: `non_idempotent`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "summary": {
      "type": "string",
      "maxLength": 256
    },
    "body": {
      "type": "string",
      "maxLength": 4096
    }
  },
  "required": [
    "summary"
  ],
  "additionalProperties": false
}
```

## `network.status`

Read NetworkManager connectivity and state; no privileged reconfiguration.

Idempotency: `read_only`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {},
  "required": [],
  "additionalProperties": false
}
```

## `systemd.user.status`

Inspect a named user service through systemd D-Bus.

Idempotency: `read_only`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "unit": {
      "type": "string",
      "maxLength": 255,
      "pattern": "^[a-zA-Z0-9_.@:-]+\\.service$"
    }
  },
  "required": [
    "unit"
  ],
  "additionalProperties": false
}
```

## `blender.status`

Connection and host version.

Idempotency: `read_only`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {},
  "required": [],
  "additionalProperties": false
}
```

## `blender.scene.inspect`

Inspect scene objects, collections, materials and render settings.

Idempotency: `read_only`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {},
  "required": [],
  "additionalProperties": false
}
```

## `blender.object.list`

List native objects without scraping the GUI.

Idempotency: `read_only`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {},
  "required": [],
  "additionalProperties": false
}
```

## `blender.object.get`

Read one object by exact name.

Idempotency: `read_only`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "name": {
      "type": "string",
      "maxLength": 128
    }
  },
  "required": [
    "name"
  ],
  "additionalProperties": false
}
```

## `blender.object.create`

Create an allowlisted primitive.

Idempotency: `non_idempotent`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "name": {
      "type": "string",
      "maxLength": 128
    },
    "primitive": {
      "type": "string",
      "enum": [
        "cube",
        "uv_sphere",
        "cylinder",
        "plane",
        "empty"
      ]
    },
    "location": {
      "type": "array",
      "items": {
        "type": "number",
        "minimum": -1000000.0,
        "maximum": 1000000.0
      },
      "maxItems": 3,
      "minItems": 3
    }
  },
  "required": [
    "name",
    "primitive"
  ],
  "additionalProperties": false
}
```

## `blender.object.delete`

Delete an exact named object.

Idempotency: `destructive`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "name": {
      "type": "string",
      "maxLength": 128
    }
  },
  "required": [
    "name"
  ],
  "additionalProperties": false
}
```

## `blender.object.transform`

Set absolute object transforms.

Idempotency: `idempotent`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "name": {
      "type": "string",
      "maxLength": 128
    },
    "location": {
      "type": "array",
      "items": {
        "type": "number",
        "minimum": -1000000.0,
        "maximum": 1000000.0
      },
      "maxItems": 3,
      "minItems": 3
    },
    "rotation": {
      "type": "array",
      "items": {
        "type": "number",
        "minimum": -1000000.0,
        "maximum": 1000000.0
      },
      "maxItems": 3,
      "minItems": 3
    },
    "scale": {
      "type": "array",
      "items": {
        "type": "number",
        "minimum": -1000000.0,
        "maximum": 1000000.0
      },
      "maxItems": 3,
      "minItems": 3
    }
  },
  "required": [
    "name"
  ],
  "additionalProperties": false
}
```

## `blender.collection.list`

List collections.

Idempotency: `read_only`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {},
  "required": [],
  "additionalProperties": false
}
```

## `blender.collection.create`

Create a collection; conflicting names fail.

Idempotency: `non_idempotent`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "name": {
      "type": "string",
      "maxLength": 128
    }
  },
  "required": [
    "name"
  ],
  "additionalProperties": false
}
```

## `blender.collection.link`

Link an object into an existing collection.

Idempotency: `idempotent`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "object": {
      "type": "string",
      "maxLength": 128
    },
    "collection": {
      "type": "string",
      "maxLength": 128
    }
  },
  "required": [
    "object",
    "collection"
  ],
  "additionalProperties": false
}
```

## `blender.material.list`

List materials.

Idempotency: `read_only`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {},
  "required": [],
  "additionalProperties": false
}
```

## `blender.material.create`

Create a Principled material with typed color/roughness.

Idempotency: `non_idempotent`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "name": {
      "type": "string",
      "maxLength": 128
    },
    "color": {
      "type": "array",
      "items": {
        "type": "number",
        "minimum": 0,
        "maximum": 1
      },
      "maxItems": 4,
      "minItems": 4
    },
    "roughness": {
      "type": "number",
      "minimum": 0,
      "maximum": 1
    },
    "metallic": {
      "type": "number",
      "minimum": 0,
      "maximum": 1
    }
  },
  "required": [
    "name"
  ],
  "additionalProperties": false
}
```

## `blender.material.assign`

Assign one existing material to an exact object.

Idempotency: `idempotent`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "object": {
      "type": "string",
      "maxLength": 128
    },
    "material": {
      "type": "string",
      "maxLength": 128
    }
  },
  "required": [
    "object",
    "material"
  ],
  "additionalProperties": false
}
```

## `blender.render.settings`

Set bounded resolution and sample settings.

Idempotency: `idempotent`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "width": {
      "type": "integer",
      "minimum": 16,
      "maximum": 8192
    },
    "height": {
      "type": "integer",
      "minimum": 16,
      "maximum": 8192
    },
    "samples": {
      "type": "integer",
      "minimum": 1,
      "maximum": 4096
    },
    "engine": {
      "type": "string",
      "enum": [
        "CYCLES",
        "BLENDER_EEVEE_NEXT"
      ]
    }
  },
  "required": [],
  "additionalProperties": false
}
```

## `blender.render`

Render to a path below the owner-configured Blender workspace.

Idempotency: `non_idempotent`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "path": {
      "type": "string",
      "maxLength": 4096
    }
  },
  "required": [
    "path"
  ],
  "additionalProperties": false
}
```

## `blender.file.save`

Save the current scene under the owner-configured workspace.

Idempotency: `destructive`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "path": {
      "type": "string",
      "maxLength": 4096
    }
  },
  "required": [
    "path"
  ],
  "additionalProperties": false
}
```

## `blender.file.open`

Open an existing .blend with automatic Python execution disabled.

Idempotency: `destructive`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "path": {
      "type": "string",
      "maxLength": 4096
    }
  },
  "required": [
    "path"
  ],
  "additionalProperties": false
}
```

## `browser.status`

Inspect the project-owned Chromium instance.

Idempotency: `read_only`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {},
  "required": [],
  "additionalProperties": false
}
```

## `browser.launch`

Launch an isolated Chromium profile; no attachment to normal profiles.

Idempotency: `non_idempotent`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "headless": {
      "type": "boolean"
    }
  },
  "required": [],
  "additionalProperties": false
}
```

## `browser.tab.list`

List targets in the isolated browser.

Idempotency: `read_only`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {},
  "required": [],
  "additionalProperties": false
}
```

## `browser.tab.open`

Create an isolated browser tab.

Idempotency: `non_idempotent`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "url": {
      "type": "string",
      "maxLength": 8192
    }
  },
  "required": [
    "url"
  ],
  "additionalProperties": false
}
```

## `browser.tab.close`

Close one exact browser tab.

Idempotency: `destructive`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "ref": {
      "type": "string",
      "maxLength": 80,
      "pattern": "^(ui|win|app|dom|tab|screen|process):[0-9a-f]{32}$"
    }
  },
  "required": [
    "ref"
  ],
  "additionalProperties": false
}
```

## `browser.tab.focus`

Activate a browser tab.

Idempotency: `idempotent`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "ref": {
      "type": "string",
      "maxLength": 80,
      "pattern": "^(ui|win|app|dom|tab|screen|process):[0-9a-f]{32}$"
    }
  },
  "required": [
    "ref"
  ],
  "additionalProperties": false
}
```

## `browser.navigate`

Navigate a tab to an http(s) URL; file URLs are denied.

Idempotency: `non_idempotent`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "ref": {
      "type": "string",
      "maxLength": 80,
      "pattern": "^(ui|win|app|dom|tab|screen|process):[0-9a-f]{32}$"
    },
    "url": {
      "type": "string",
      "maxLength": 8192
    }
  },
  "required": [
    "ref",
    "url"
  ],
  "additionalProperties": false
}
```

## `browser.dom.snapshot`

Get a bounded DOM tree without evaluating arbitrary JavaScript.

Idempotency: `read_only`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "ref": {
      "type": "string",
      "maxLength": 80,
      "pattern": "^(ui|win|app|dom|tab|screen|process):[0-9a-f]{32}$"
    },
    "depth": {
      "type": "integer",
      "minimum": 0,
      "maximum": 8
    }
  },
  "required": [
    "ref"
  ],
  "additionalProperties": false
}
```

## `browser.dom.query`

Query DOM nodes using a CSS selector; return candidates, not an implicit click.

Idempotency: `read_only`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "ref": {
      "type": "string",
      "maxLength": 80,
      "pattern": "^(ui|win|app|dom|tab|screen|process):[0-9a-f]{32}$"
    },
    "selector": {
      "type": "string",
      "maxLength": 1024
    }
  },
  "required": [
    "ref",
    "selector"
  ],
  "additionalProperties": false
}
```

## `browser.dom.click`

Click one exact DOM reference after document-generation validation.

Idempotency: `non_idempotent`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "ref": {
      "type": "string",
      "maxLength": 80,
      "pattern": "^(ui|win|app|dom|tab|screen|process):[0-9a-f]{32}$"
    }
  },
  "required": [
    "ref"
  ],
  "additionalProperties": false
}
```

## `browser.dom.fill`

Replace a text field using DOM focus and typed CDP input.

Idempotency: `non_idempotent`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "ref": {
      "type": "string",
      "maxLength": 80,
      "pattern": "^(ui|win|app|dom|tab|screen|process):[0-9a-f]{32}$"
    },
    "text": {
      "type": "string",
      "maxLength": 65536
    }
  },
  "required": [
    "ref",
    "text"
  ],
  "additionalProperties": false
}
```

## `browser.screenshot`

Capture an isolated tab into a private artifact.

Idempotency: `non_idempotent`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "ref": {
      "type": "string",
      "maxLength": 80,
      "pattern": "^(ui|win|app|dom|tab|screen|process):[0-9a-f]{32}$"
    }
  },
  "required": [
    "ref"
  ],
  "additionalProperties": false
}
```

## `browser.downloads.status`

Inspect tracked download events for the isolated browser.

Idempotency: `read_only`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {},
  "required": [],
  "additionalProperties": false
}
```

## `browser.diagnostics`

Return bounded event counts, not network headers or console payloads.

Idempotency: `read_only`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {},
  "required": [],
  "additionalProperties": false
}
```

## `plugin.install`

Install a digest-pinned manifest into this broker session after external approval

Idempotency: `non_idempotent`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "manifest": {
      "type": "object",
      "maxProperties": 20
    }
  },
  "required": [
    "manifest"
  ],
  "additionalProperties": false
}
```

## `plugin.remove`

Remove a plugin and its registered commands from this broker

Idempotency: `non_idempotent`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "name": {
      "type": "string",
      "maxLength": 40
    }
  },
  "required": [
    "name"
  ],
  "additionalProperties": false
}
```

## `plugin.doctor`

Validate a plugin manifest, digest and sandbox prerequisites

Idempotency: `read_only`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "name": {
      "type": "string",
      "maxLength": 40
    }
  },
  "required": [
    "name"
  ],
  "additionalProperties": false
}
```

## `capabilities.search`

Search the bounded capability catalog by exact ID, words, quoted phrases, provider, app, risk, category, tags, object types and current operation availability. Results include provenance, not blanket permission.

Idempotency: `read_only`. Dry run: `true`.

```json
{
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "query": {
      "type": "string",
      "maxLength": 256
    },
    "provider": {
      "type": [
        "string",
        "null"
      ],
      "maxLength": 256
    },
    "app": {
      "type": [
        "string",
        "null"
      ],
      "maxLength": 256
    },
    "category": {
      "type": [
        "string",
        "null"
      ],
      "maxLength": 256
    },
    "risk": {
      "enum": [
        "code_execution",
        "destructive",
        "mutating",
        "mutating_reversible",
        "privilege_sensitive",
        "read_only",
        "secret_access",
        null
      ]
    },
    "available": {
      "type": [
        "boolean",
        "null"
      ]
    },
    "tags": {
      "type": "array",
      "maxItems": 16,
      "items": {
        "type": "string",
        "maxLength": 128
      }
    },
    "object_types": {
      "type": "array",
      "maxItems": 16,
      "items": {
        "type": "string",
        "maxLength": 128
      }
    },
    "limit": {
      "type": "integer",
      "minimum": 1,
      "maximum": 100
    },
    "offset": {
      "type": "integer",
      "minimum": 0,
      "maximum": 8192
    },
    "revision": {
      "type": [
        "integer",
        "null"
      ],
      "minimum": 0
    },
    "source": {
      "enum": [
        "builtin",
        "plugin",
        "driver",
        "external_mcp",
        "recipe",
        null
      ]
    }
  }
}
```

## `capabilities.describe`

Describe one capability with full input/output schema, source digest, provenance and operation-specific routes. Availability never grants authorization.

Idempotency: `read_only`. Dry run: `true`.

```json
{
  "type": "object",
  "additionalProperties": false,
  "required": [
    "name"
  ],
  "properties": {
    "name": {
      "type": "string",
      "minLength": 1,
      "maxLength": 128
    }
  }
}
```

## `jobs.start`

Start a bounded session-scoped job; the nested command is independently authorized and audited.

Idempotency: `non_idempotent`. Dry run: `false`.

```json
{
  "type": "object",
  "required": [
    "request"
  ],
  "properties": {
    "request": {
      "type": "object",
      "required": [
        "command"
      ],
      "properties": {
        "command": {
          "type": "string",
          "minLength": 1,
          "maxLength": 128
        },
        "args": {
          "type": "object",
          "maxProperties": 128
        },
        "dry_run": {
          "type": "boolean"
        },
        "backend": {
          "type": [
            "string",
            "null"
          ],
          "maxLength": 128
        }
      },
      "additionalProperties": false
    }
  },
  "additionalProperties": false
}
```

## `jobs.list`

List retained jobs owned by this broker session, newest first, including bounded progress and artifact metadata.

Idempotency: `read_only`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {},
  "additionalProperties": false
}
```

## `jobs.get`

Read one job owned by this broker session without exposing jobs from other sessions.

Idempotency: `read_only`. Dry run: `true`.

```json
{
  "type": "object",
  "required": [
    "job_id"
  ],
  "properties": {
    "job_id": {
      "type": "string",
      "pattern": "^[0-9a-f]{32}$"
    }
  },
  "additionalProperties": false
}
```

## `jobs.cancel`

Request cancellation of one job owned by this broker session; repeated cancellation is idempotent.

Idempotency: `idempotent`. Dry run: `false`.

```json
{
  "type": "object",
  "required": [
    "job_id"
  ],
  "properties": {
    "job_id": {
      "type": "string",
      "pattern": "^[0-9a-f]{32}$"
    }
  },
  "additionalProperties": false
}
```

## `workflow.candidate.delete`

Delete one compiled workflow candidate after any promotion is demoted.

Idempotency: `destructive`. Dry run: `false`.

```json
{
  "type": "object",
  "properties": {
    "candidate_id": {
      "type": "string",
      "minLength": 7,
      "maxLength": 96
    }
  },
  "required": [
    "candidate_id"
  ],
  "additionalProperties": false
}
```

## `workflow.candidate.get`

Inspect a compiled workflow candidate.

Idempotency: `read_only`. Dry run: `false`.

```json
{
  "type": "object",
  "properties": {
    "candidate_id": {
      "type": "string",
      "minLength": 10,
      "maxLength": 96
    }
  },
  "required": [
    "candidate_id"
  ],
  "additionalProperties": false
}
```

## `workflow.compile`

Compile 1..8 explicit compatible traces into a deterministic Recipe v1 candidate.

Idempotency: `non_idempotent`. Dry run: `false`.

```json
{
  "type": "object",
  "properties": {
    "trace_ids": {
      "type": "array",
      "items": {
        "type": "string",
        "minLength": 7,
        "maxLength": 80
      },
      "minItems": 1,
      "maxItems": 8,
      "uniqueItems": true
    },
    "name": {
      "type": "string",
      "minLength": 1,
      "maxLength": 80
    },
    "description": {
      "type": "string",
      "maxLength": 4096
    },
    "parameters": {
      "type": "array",
      "maxItems": 64,
      "items": {
        "type": "object",
        "properties": {
          "name": {
            "type": "string",
            "minLength": 1,
            "maxLength": 80
          },
          "step": {
            "type": "integer",
            "minimum": 0,
            "maximum": 63
          },
          "pointer": {
            "type": "string",
            "maxLength": 512
          },
          "secret": {
            "type": "boolean"
          }
        },
        "required": [
          "name",
          "step",
          "pointer"
        ],
        "additionalProperties": false
      }
    }
  },
  "required": [
    "trace_ids",
    "name"
  ],
  "additionalProperties": false
}
```

## `workflow.demote`

Remove a promoted learned workflow from the live capability catalog.

Idempotency: `non_idempotent`. Dry run: `false`.

```json
{
  "type": "object",
  "properties": {
    "slug": {
      "type": "string",
      "pattern": "^[a-z][a-z0-9-]{0,38}[a-z0-9]$",
      "maxLength": 40
    }
  },
  "required": [
    "slug"
  ],
  "additionalProperties": false
}
```

## `workflow.promote`

Promote a statically verified and successfully replayed candidate into a searchable recipe capability.

Idempotency: `non_idempotent`. Dry run: `false`.

```json
{
  "type": "object",
  "properties": {
    "candidate_id": {
      "type": "string",
      "minLength": 10,
      "maxLength": 96
    },
    "slug": {
      "type": "string",
      "pattern": "^[a-z][a-z0-9-]{0,38}[a-z0-9]$",
      "maxLength": 40
    }
  },
  "required": [
    "candidate_id",
    "slug"
  ],
  "additionalProperties": false
}
```

## `workflow.promotions.list`

List promoted learned workflow capabilities and drift status.

Idempotency: `read_only`. Dry run: `false`.

```json
{
  "type": "object",
  "properties": {},
  "required": [],
  "additionalProperties": false
}
```

## `workflow.record.start`

Start explicit local workflow recording for this session; value capture is opt-in.

Idempotency: `non_idempotent`. Dry run: `false`.

```json
{
  "type": "object",
  "properties": {
    "name": {
      "type": "string",
      "minLength": 1,
      "maxLength": 80
    },
    "intent": {
      "type": "string",
      "maxLength": 4096
    },
    "capture_values": {
      "type": "boolean",
      "default": false
    }
  },
  "required": [
    "name"
  ],
  "additionalProperties": false
}
```

## `workflow.record.stop`

Stop explicit workflow recording and persist the bounded trace.

Idempotency: `non_idempotent`. Dry run: `false`.

```json
{
  "type": "object",
  "properties": {
    "successful": {
      "type": "boolean",
      "default": true
    }
  },
  "required": [],
  "additionalProperties": false
}
```

## `workflow.replay`

Replay a verified workflow candidate through the broker; every step is re-authorized.

Idempotency: `non_idempotent`. Dry run: `true`.

```json
{
  "type": "object",
  "properties": {
    "candidate_id": {
      "type": "string",
      "minLength": 10,
      "maxLength": 96
    },
    "inputs": {
      "type": "object",
      "maxProperties": 64,
      "additionalProperties": true
    }
  },
  "required": [
    "candidate_id"
  ],
  "additionalProperties": false
}
```

## `workflow.trace.delete`

Delete one local recorded workflow trace when no candidate references it.

Idempotency: `destructive`. Dry run: `false`.

```json
{
  "type": "object",
  "properties": {
    "trace_id": {
      "type": "string",
      "minLength": 7,
      "maxLength": 96
    }
  },
  "required": [
    "trace_id"
  ],
  "additionalProperties": false
}
```

## `workflow.trace.get`

Inspect one recorded workflow trace.

Idempotency: `read_only`. Dry run: `false`.

```json
{
  "type": "object",
  "properties": {
    "trace_id": {
      "type": "string",
      "minLength": 7,
      "maxLength": 80
    }
  },
  "required": [
    "trace_id"
  ],
  "additionalProperties": false
}
```

## `workflow.traces.list`

List locally recorded workflow traces without replaying them.

Idempotency: `read_only`. Dry run: `false`.

```json
{
  "type": "object",
  "properties": {},
  "required": [],
  "additionalProperties": false
}
```

## `workflow.verify`

Statically verify a workflow candidate against current capability schemas and descriptor digests.

Idempotency: `non_idempotent`. Dry run: `false`.

```json
{
  "type": "object",
  "properties": {
    "candidate_id": {
      "type": "string",
      "minLength": 10,
      "maxLength": 96
    }
  },
  "required": [
    "candidate_id"
  ],
  "additionalProperties": false
}
```
