# Command reference

Generated from `schemas/commands.json`; do not edit by hand.

83 built-in descriptors. A descriptor is not proof of live backend support.
Run `computerctl doctor` and consult `compatibility.md` and `../VERIFY.md`.

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
| `app.list` | `app.observe` | read_only | 10000 ms | atspi |
| `app.launch` | `app.launch` | code_execution | 10000 ms | system |
| `app.close` | `app.close`, `window.manage` | destructive | 10000 ms | sway, hyprland, gnome, kwin, x11 |
| `window.list` | `window.observe` | read_only | 10000 ms | sway, hyprland, gnome, kwin, x11 |
| `window.focus` | `window.manage` | mutating_reversible | 10000 ms | sway, hyprland, gnome, kwin, x11 |
| `window.move` | `window.manage` | mutating_reversible | 10000 ms | sway, hyprland, gnome, kwin, x11 |
| `window.resize` | `window.manage` | mutating_reversible | 10000 ms | sway, hyprland, gnome, kwin, x11 |
| `window.close` | `window.manage` | destructive | 10000 ms | sway, hyprland, gnome, kwin, x11 |
| `ui.snapshot` | `ui.observe` | read_only | 10000 ms | atspi |
| `ui.find` | `ui.observe` | read_only | 10000 ms | core |
| `ui.invoke` | `ui.invoke` | mutating | 10000 ms | atspi |
| `ui.set_text` | `ui.invoke` | mutating | 10000 ms | atspi |
| `ui.read_text` | `ui.text.read` | secret_access | 10000 ms | atspi |
| `ui.set_value` | `ui.invoke` | mutating_reversible | 10000 ms | atspi |
| `ui.get_value` | `ui.observe` | read_only | 10000 ms | atspi |
| `ui.toggle` | `ui.invoke` | mutating | 10000 ms | atspi |
| `ui.select` | `ui.invoke` | mutating | 10000 ms | atspi |
| `ui.expand` | `ui.invoke` | mutating | 10000 ms | atspi |
| `portal.start` | `input.keyboard`, `input.pointer` | privilege_sensitive | 120000 ms | portal |
| `portal.stop` | `desktop.observe` | mutating_reversible | 10000 ms | portal |
| `portal.status` | `desktop.observe` | read_only | 10000 ms | portal |
| `input.key` | `input.keyboard` | mutating | 10000 ms | portal, x11 |
| `input.type` | `input.keyboard` | mutating | 10000 ms | portal |
| `pointer.move` | `input.pointer` | mutating | 10000 ms | portal, x11 |
| `pointer.click` | `input.pointer` | mutating | 10000 ms | portal, x11 |
| `pointer.scroll` | `input.pointer` | mutating | 10000 ms | portal |
| `screen.capture` | `screen.capture` | secret_access | 120000 ms | portal |
| `screen.stream_info` | `desktop.observe` | read_only | 10000 ms | portal |
| `clipboard.read` | `clipboard.read` | secret_access | 10000 ms | clipboard |
| `clipboard.write` | `clipboard.write` | mutating | 10000 ms | clipboard |
| `process.list` | `process.observe` | read_only | 10000 ms | system |
| `process.signal` | `process.manage` | destructive | 10000 ms | system |
| `filesystem.read` | `filesystem.read` | read_only | 10000 ms | filesystem |
| `filesystem.write` | `filesystem.write` | mutating_reversible | 10000 ms | filesystem |
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

Bounded semantic accessibility snapshot. No screenshot or OCR.

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

Request an ephemeral RemoteDesktop session; the desktop presents native user consent.

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

Inspect consent state and portal interface versions.

Idempotency: `read_only`. Dry run: `true`.

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

Describe portal ScreenCast support; this build does not decode PipeWire frames.

Idempotency: `read_only`. Dry run: `true`.

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
