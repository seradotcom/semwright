# Workflow distillation v1 + v2

Workflow distillation turns explicit, successful Semwright executions into versioned
Recipe v1 candidates and, after verification and successful replay, into normal searchable
capabilities. V1 is deliberately user-directed. V2 adds deterministic repeated-pattern
mining over those same explicitly recorded traces; it never records background activity.

The lifecycle is:

```text
record → compile → verify → replay → promote
                                  ↘ demote
```

V1 remains the trust boundary for compilation, replay and promotion. V2 proposes
repeated structures. V3 derives an in-memory compiled proposal from strong V2 evidence,
runs static Recipe validation automatically, and exposes only sanitized proposal metadata.
The candidate is not persisted until explicit acceptance and still needs a successful live
replay plus explicit promotion before it can enter the normal capability catalog.

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

Compiled candidates remain discoverable across daemon restarts before promotion:

```sh
semwright workflow candidates
semwright workflow candidate CANDIDATE_ID
```

The list reports static-verification state, successful live-replay evidence and current
descriptor-drift status without exposing captured arguments or results.

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

## V2 repeated-pattern mining

V2 derives patterns only from traces already captured through the explicit V1 recorder.
It does not enable passive observation. Pattern identity is a SHA-256 over command order,
descriptor/version/risk metadata and bounded argument/result **shape**. Scalar values are
never embedded in the fingerprint or suggestion payload.

The default suggestion threshold is three successful structurally compatible traces:

```sh
semwright workflow patterns
semwright workflow suggestions
semwright workflow suggestion SUGGESTION_ID
```

A pattern reports command sequence, occurrence counts, compile-ready evidence and opaque
location digests for arguments that varied across value-capturing observations. It never
reports the observed values or raw JSON pointers/object keys. Metadata-only traces can
contribute repetition evidence but cannot be used to compile a candidate.

Suggestions are advisory. A suggestion needs at least two compatible, successful,
unredacted value-capturing traces before:

```sh
semwright workflow compile-suggestion SUGGESTION_ID
```

That command reuses the V1 compiler; it does not create a second compilation or authority
path. The resulting candidate still requires `workflow.verify`, a successful live replay
and explicit `workflow.promote` before it appears as a normal capability.

Suggestions may be dismissed. A temporary dismissal resurfaces only after new matching
evidence arrives; a permanent dismissal stays hidden until explicitly restored. Only
dismissal state is persisted. Patterns and suggestions are recomputed from canonical
traces so they cannot drift into a second source of truth.

When a pattern reaches the default threshold for the first time, Semwright emits a bounded,
session-scoped `workflow.pattern.detected` event containing only IDs, occurrence count and
compile-readiness metadata; unrelated sessions cannot observe that learning notification.

## V2 non-goals

V2 still does not:

- watch unrecorded user or application activity;
- infer task boundaries from ambient sessions;
- use an LLM to guess workflow intent, parameters or postconditions;
- auto-compile a suggestion merely because it repeated;
- auto-verify, auto-replay or auto-promote a learned capability;
- expose raw observed values through pattern/suggestion metadata;
- claim ACID transactions or rollback;
- store passwords/secret-access results for learning.

Automatic candidate proposal is reserved for V3 and must preserve the same broker,
policy, replay and promotion gates.
## V3 automatic proposals

V3 is deliberately **automatic about analysis, not authority**. A proposal is eligible only
when at least three compatible value-capturing traces are available. Semwright compiles the
latest bounded evidence set in memory with the V1 compiler, checks descriptor drift and
validates Recipe v1 statically. The public proposal contains command names, inferred input
types, aggregate permissions/risk and evidence counts, but never the compiled recipe,
captured constants, raw pointers or source trace IDs.

```sh
semwright workflow proposals
semwright workflow proposal PROPOSAL_ID
semwright workflow plan-proposal PROPOSAL_ID --args-json '{"input":"value"}'
semwright workflow accept-proposal PROPOSAL_ID
```

Proposal identity includes the compiled candidate fingerprint. New evidence that changes
the compilation produces a different proposal ID, so accepting a stale proposal fails
instead of silently accepting a newer draft.

Evidence tiers (`standard`, `strong`, `very_strong`) are deterministic heuristics over
occurrence and compile-ready counts. They are **not probabilities of success** and never
affect policy authority.

`workflow.proposal.plan` always invokes Recipe v1 in dry-run mode. `accept-proposal`
persists the exact candidate and records its already-completed static verification, but
does not execute it. The accepted candidate still needs a successful explicit
`workflow replay` before `workflow promote` can succeed.

When the third compile-ready observation arrives, Semwright emits a session-scoped
`workflow.proposal.ready` event. No proposal is persisted merely because that event
exists.

V3 intentionally does not auto-replay mutating operations, auto-promote capabilities,
infer permissions from model confidence, or use an LLM as an authorization source.
