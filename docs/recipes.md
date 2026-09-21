# Declarative recipes v1

A recipe is structured data, not a prompt, shell program or JavaScript template.
The current format is `version: 1` rather than the blueprint's illustrative apiVersion/
kind wrapper. Rust accepts YAML or JSON and rejects unknown fields. The parser limits a
recipe document to 256 KiB. Validation limits names, inputs, outputs, steps, assertions,
expression depth and retry budgets. Definitions and examples are checked in under
`recipes/` and `schemas/recipe.schema.json`.

```yaml
version: 1
name: inspect-export
inputs: {}
steps:
  - id: find
    command: ui.find
    args:
      selector:
        role: button
        name: {op: exact, value: Export}
    timeout_ms: 10000
    assertions:
      - left: {$var: /steps/find/count}
        op: equals
        right: 1
  - id: invoke
    command: ui.invoke
    args:
      ref: {$var: /steps/find/nodes/0/ref}
    timeout_ms: 10000
outputs:
  selected:
    kind: string
    value: {$var: /steps/find/nodes/0/ref}
```

A variable binding is an **entire object** with exactly one `$var` JSON pointer to `/inputs/`
or `/steps/`. It preserves the value type. `${...}` and shell-like strings are ordinary
strings. Each input declares `kind` (`string`, `number`, `integer`, `boolean`, `object`,
`array`), optional `default`, and optional `secret`. Outputs declare kind/value/secret.
Secret-marked input and sensitive step outputs are conservatively redacted when returned;
this is not a formal guarantee against timing/control-flow disclosure.

Steps support `when`, assertions with equals/not_equals/truthy/exists/count_equals,
positive timeout no greater than the command timeout, and explicit retries. Attempts are
limited to 1–5, backoff to 5,000 ms. Retries are permitted only for commands classified
read-only/idempotent and appropriate known-outcome errors. Destructive/non-idempotent
commands may not acquire retries through a recipe. Every step re-enters authorization.
Nested recipe commands are rejected.

```sh
computerctl recipe validate recipes/fake-export.yaml
computerctl --dry-run recipe run recipes/fake-export.yaml
computerctl recipe test recipes/fake-export.yaml --backend fake
computerctl recipe scaffold my-flow ./my-flow.yaml
```

Validation/dry-run do not prove future object identity or permission. A dry-run recipe
returns a structural plan without executing prior steps; it cannot discover refs that do
not exist yet. A failure can leave earlier effects intact. Errors include progress for
several common paths but full uniform progress reporting remains incomplete. There is no
ACID transaction or automatic compensation/rollback claim. Inspect current state before
resuming. `recipe test` insists on an explicitly fake broker; it does not launch one.
