# Architecture

Semwright separates application semantics from operating-system enforcement. CLI, MCP,
the inspector and recipes converge on the broker. A frontend, provider or Skill does not
obtain another authority path.

```text
Untrusted intent / Skill prose / observed application data
                         |
                 CLI / MCP / recipes
                         |
       Broker: schemas, policy, approval, refs, jobs, audit
                         |
          Capability Registry / Provider Runtime
                         |
      platform hosts / Driver Host / federated MCP
      Linux, macOS, Windows / pinned tools and sessions
                         |
                    applications
```

## Authority and platform boundary

`types`, `registry`, `policy` and `backend-api` define shared data, descriptor validation,
permission intent and provider contracts. `core` owns broker dispatch, provider leases,
refs, jobs and audit. The daemon composes these services and owner-configured providers.
OS implementations live in `platform-linux[-sys]`, `platform-macos[-sys]` and
`platform-windows[-sys]`, behind `platform-api`, `platform-common`, `platform-host`
and `platform-services`.

Portability is not an equivalence claim. Linux retains pinned-root/openat2 confinement
and Bubblewrap/Landlock launch admission. Windows uses native filesystem, IPC and
restricted/AppContainer mechanisms with explicit unsupported cases. macOS has native
host services and executable verification; arbitrary Driver/Plugin Host execution remains
fail-closed. Interactive TCC/UIPI/portal acceptance is distinct from native CI.
See [platforms](platforms.md) and [security](security.md).

## Dispatch and identity

The broker validates the selected descriptor and provenance, evaluates policy and scope,
obtains its execution/approval gate, and revalidates live references before dispatch.
Cancellation, deadlines and uncertain outcomes are part of the result contract. A failed
mutation is not silently replayed through another backend.

Operational refs are opaque and session-scoped. Generations, fingerprints and expiry
prevent known stale identities from silently retargeting work. Persistent Graph identity
serves a different purpose: a stored project node is not a live reference or permission.
Provider disconnect/catalog changes invalidate the relevant runtime identity.

## Provider Runtime and Driver Host

Providers have owner-assigned identity and provenance. Dynamic catalogs are revisioned;
an external provider cannot claim the builtin namespace. Federation imports untrusted
schemas, descriptions and results through the same policy path.

The Driver SDK defines application-facing contracts, not broker policy. Driver Host verifies
and pins executables and constructs supported platform launch profiles. One-shot tools,
user-session-scoped detached jobs and provider-scoped persistent sessions have different
lifetimes. Logical mounts, tools and dependencies are resolved by the Host, not by ambient
executable discovery inside a driver. See [runtime tools](runtime-tools.md).

Provider-scoped runtime sessions are deliberately not private per-user-session stores.
Their handle is not a policy grant. Callers still enter an authorized provider operation;
the Host enforces tool/mount contracts and resource bounds. Sandboxing a helper process
does not isolate an already-running external application.

## Composition, artifacts, Graph and Effects

`semantic-composition` provides typed plans, owner binding, PlanVault/controller, bounded
attempts and reconciliation records. Application profiles realize them through their
providers; they do not create another kernel or bypass the broker.

`media-time`, audio/video domains and `av-composition` carry shared time, artifact and
publication contracts. AV preserves owner, source, scope and artifact provenance through
native execution and readback. Acceptance of an operation is not automatically persistence
or verification of its result.

`project-graph` records persistent identity, dependencies, derived artifacts, drift and
reconciliation. CURRENT/STALE/UNKNOWN describe evidence for a declared relationship.
`effect-conformance` evaluates predicates and observation quality; incomplete enumeration
cannot establish a global absence-of-change result. Read the
[integrated contracts and evidence](semantic-creation/INTEGRATION.md) for concrete consumers.

## Skills, recipes and workflows

Skills are untrusted procedural knowledge interpreted by the agent, not executable broker
authority. Optional compatibility/lock metadata detects descriptor drift but grants no
permissions. Skill scripts are not automatically run. Recipes re-enter the broker at every
step. Workflow Distillation can produce a verified recipe capability without turning an
observation or model instruction into a grant. Recipes are not transactions.

See [Skills](skills.md), [recipes](recipes.md), [Workflow Distillation](workflow-distillation.md),
[events/jobs](events-jobs.md), [permissions](permissions.md) and [verification](../VERIFY.md).
