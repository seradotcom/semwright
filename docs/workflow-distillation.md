# Workflow distillation v1

Workflow distillation turns one or more explicit, successful Semwright executions into a
versioned Recipe v1 candidate and, after verification and a successful replay, into a
normal searchable capability. V1 is deliberately user-directed: it records only between
explicit start/stop commands and never mines background activity.

The lifecycle is:

```text
record → compile → verify → replay → promote
                                  ↘ demote
```

V2 pattern mining and V3 automatic candidate generation are intentionally out of scope
until this path is verified.

## Authorization

Workflow administration is not covered by `desktop.observe`. Owner policy must explicitly
grant:

- `workflow.record` — record, inspect, compile, verify and replay local candidates.
- `workflow.manage` — promote/demote and delete learned workflow state.

A promoted capability does **not** retain either administrative scope. Its descriptor
contains the union of the underlying step permissions and the maximum step risk.
Every recipe step re-enters the normal broker, policy, audit and provider selection path.
Promotion cannot convert an operation into broader authority.

## Recording and privacy

Recording is session-scoped and bounded to 64 operational steps. Commands that manage
workflows, recipes, audit, discovery, jobs, events and plugins are not recursively
recorded.

By default `capture_values=false`. Non-reference values are redacted, making the trace
useful for diagnostics but intentionally not compilable. Compilation requires an explicit
value-capturing trace:

```sh
semwright workflow record start export-demo --capture-values
# perform the task through Semwright
semwright workflow record stop
```

Even with value capture enabled, fields whose names look like credentials/tokens and all
`secret_access` capability payloads are redacted. An operation that cannot be recorded
within the configured size/depth budgets invalidates the trace instead of being silently
omitted.
The local workflow store is private state. On Unix its directory is mode 0700 and its
file is mode 0600; symlinked stores are rejected. Writes use a same-directory temporary
file, fsync, rename and directory fsync. The store is bounded to 8 MiB. On restore,
candidate fingerprints are recomputed, descriptor-digest coverage must exactly match the
recipe commands, every source trace must still exist, and promoted state must resolve to
a verified/replayed canonical candidate. Corrupt or divergent persisted state fails
closed instead of being registered as a learned capability.

## Compile

`workflow.compile` accepts 1–8 explicit successful traces with identical command
sequences. Every trace must:

- have captured values;
- contain no redacted or failed step;
- have known outcomes;
- still match the current capability descriptor digest/version/risk/idempotency.

With multiple traces, scalar argument values that differ are converted into typed recipe
inputs. Values that correspond to a unique prior step result are rewritten as `$var`
bindings so ephemeral state is reacquired during replay. V1 recognizes only compatible
structural dataflow classes — opaque refs, identities, filesystem paths/roots, SHA-256
digests and revisions — so patterns such as
`resulting_revision → expected_revision` and `artifact.path → source_path` compile
without binding unrelated equal strings. An opaque reference that cannot be tied to a
prior result is never baked into a recipe: compilation fails until the caller explicitly
parameterizes it.

With a single trace, constants remain constants unless the caller supplies an explicit
parameter hint:

```sh
semwright workflow compile export   --trace TRACE_ID   --parameter filename=0:/filename
```
Parameter names and source locations must be unique. A parameter hint may mark an input
as secret; because V1 cannot prove whether an application echoes that secret later, any
automatically inferred recipe output is conservatively marked secret and redacted at
runtime when the recipe has a secret input. V1 does not ask an LLM to guess which constant
should become an input.

The compiler can infer simple success assertions from stable boolean result fields, but it
does not invent application-specific postconditions.

## Verify and replay

`workflow.verify CANDIDATE` performs static Recipe v1 validation and verifies every
stored capability descriptor digest against the current catalog. A descriptor change
causes a conflict and the candidate must be recompiled/reviewed.

A live `workflow.replay` is rejected until static verification succeeds. Dry-run replay
is allowed for planning. A successful live replay increments evidence on the candidate.
No non-idempotent/destructive step receives an automatic retry merely because it was
learned.

## Promotion

Promotion requires both:

1. successful static verification; and
2. at least one successful live replay.

```sh
semwright workflow promote CANDIDATE export-mobile-assets
```

The resulting capability is:

```text
recipe.export-mobile-assets.run
```
It is registered as `SourceKind::Recipe` with learned/workflow provenance and therefore
appears in the ordinary capability catalog/search. Its input schema is derived from the
compiled recipe inputs; its output schema is bounded; risk, permissions, consent and
idempotency are aggregated conservatively from its steps.

Calling a promoted capability runs the stored Recipe v1 through the broker. A descriptor
drift check happens again before execution.

`workflow.demote SLUG` removes the capability from the live catalog without deleting
its source candidate. Trace and candidate deletion are separate destructive operations;
a referenced trace cannot be deleted until its candidates are deleted, and a promoted
candidate cannot be deleted until it is demoted.

## Restart and drift

Promotions are persisted locally. The daemon loads configured plugins, drivers and MCP
providers first, then attempts to restore learned capabilities. Promotions whose source
descriptors no longer match are reported as stale and are not registered.

This prevents a learned workflow from silently surviving an incompatible capability
change.

## V1 non-goals

V1 does not:

- watch unrecorded user activity;
- automatically segment tasks;
- search for repeated workflow patterns;
- use an LLM to generalize traces;
- auto-promote learned capabilities;
- claim ACID transactions or rollback;
- store passwords/secret-access results for learning.

Those belong to later, separately verified versions.
