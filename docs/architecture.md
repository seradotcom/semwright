# Architecture

Semwright keeps the public command model independent of D-Bus, X11, Blender, MCP and any
particular application API. CLI, MCP, recipes and the inspector all converge on the same broker;
choosing a frontend never creates a more privileged execution path.

```text
computerctl / semwright-mcp / semwright-inspect
                │ bounded Unix IPC; session identity
                ▼
             Broker
      policy / refs / audit
                │
        Capability Registry
                │
          Provider Runtime
      ┌─────────┼──────────┐
 native Linux   drivers   external MCP
 / app APIs     │          │
      └─────────┼──────────┘
                ▼
        Linux / applications
```

The Provider Runtime is the common execution boundary. A provider has explicit owner-assigned
identity, capability provenance, lifecycle and operation-level availability. Dynamic providers
cannot claim the builtin namespace. Catalog replacement is revisioned and atomic; stale catalog
pagination or capability descriptors fail rather than silently retargeting an operation.

## Provider classes

Built-in providers adapt existing Linux and application backends without rewriting them. Current
native routes include AT-SPI, compositor/window backends, portal/clipboard/system/filesystem,
Blender and private Chromium.

Federated MCP servers are dynamic `ExternalMcpProvider` instances. Their tool descriptions,
schemas and results are untrusted data. Semwright assigns the namespace, imports descriptors,
and still applies broker policy, operator approval, cancellation, provenance and audit before
delegating an invocation.

Application drivers use the same Provider Runtime. The Driver SDK defines a versioned persistent
stdio contract and the Driver Host stages a digest-pinned ELF inside bubblewrap + Landlock.
Driver manifests cannot grant themselves policy authority. Unlike the existing plugin model,
which starts one sandboxed process per invocation, a driver persists for its provider lifetime
and can maintain an application connection.

Recipes and plugins remain separate composition mechanisms: recipes re-enter broker execution
for every step; plugins provide narrow sandboxed one-shot commands.

## Execution

The broker snapshots the selected capability descriptor and provenance before dispatch. It
evaluates capability/risk/scope, obtains the execution gate, requests human approval when
required, validates current references, and invokes the selected provider with cancellation and
deadline semantics. A provider failure does not trigger an implicit retry or a hidden fallback.

References are opaque and session-scoped. Provider generations and backend fingerprints prevent
known stale objects from silently becoming newly-created objects. Dynamic provider disconnects
invalidate their catalog generation.

Provider capability discovery is not authorization. Registering a driver or MCP upstream does
not create its corresponding policy grant.

## Dependency direction

`types` owns the transport-independent domain model. `registry` validates and indexes command
descriptors. `policy` owns authorization and filesystem grants. `backend-api` owns the
Provider/Backend traits. `core` owns provider leases, broker orchestration, refs and audit.
`federation` implements MCP providers. `driver-sdk` is application-author facing and has no
broker authority; `driver-host` adapts that protocol into a sandboxed Provider. Frontends depend
on the broker/protocol contract rather than backend implementation details.

The daemon is the composition root. It creates trusted builtin providers and explicitly loads
owner-configured external providers. No root daemon, default TCP listener or automatic elevated
helper is part of the architecture. Current verification status and live-system gaps are tracked
in [VERIFY.md](../VERIFY.md) and [RELEASE_BLOCKERS.md](../RELEASE_BLOCKERS.md).
