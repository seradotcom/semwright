# Architecture

Semwright keeps its public capability model independent of the operating-system mechanism that fulfils a request. CLI, MCP, recipes and the inspector converge on the same broker; choosing a frontend never creates a more privileged execution path.

```text
semwright / semwright-mcp / semwright-inspect
                │ bounded local IPC; session identity
                ▼
             Broker
      policy / refs / audit
                │
        Capability Registry
                │
          Provider Runtime
      ┌─────────┼──────────┐
      │         │          │
 platform    drivers   external MCP
   host         │          │
      │         │          │
 ┌────┴────┐    │          │
 Linux   macOS  │          │
 └────┬────┘    │          │
      └─────────┼──────────┘
                ▼
        OS / applications
```

Linux is the currently verified live host. The macOS host foundation is experimental: native ARM64 and Intel CI compile/link the Apple-framework bridge and pass noninteractive smoke, while TCC behaviour and live desktop automation still require an authorized interactive Mac before support is claimed.

## Platform boundary

`platform-api` contains semantic contracts and data that do not expose AT-SPI, X11, AXUIElement, Mach-O handles or other native types. `platform-common` holds reusable backend-facing logic. `platform-host` is the daemon composition boundary. `platform-services` selects OS-specific filesystem, executable-verification, IPC/path and sandbox services. Linux and macOS mechanics live under `platform-linux[-sys]` and `platform-macos[-sys]`.

The boundary is deliberately not a weakest-common-denominator sandbox. Linux retains openat2, bubblewrap and Landlock enforcement. macOS is allowed to expose a different confinement level and must fail closed where the platform cannot provide an equivalent supported primitive.

High-level packages should depend on semantic contracts rather than native APIs. Platform code may depend inward on shared contracts; shared contracts must not depend on AT-SPI, X11, AppKit, ApplicationServices, ScreenCaptureKit or private OS APIs.

## Provider Runtime

The Provider Runtime is the common execution boundary. A provider has explicit owner-assigned identity, capability provenance, lifecycle and operation-level availability. Dynamic providers cannot claim the builtin namespace. Catalog replacement is revisioned and atomic; stale catalog pagination or capability descriptors fail rather than silently retargeting an operation.

Platform-native providers, application drivers and federated MCP servers all enter the same broker path. Their implementation mechanism does not grant authority.

Federated MCP servers are dynamic `ExternalMcpProvider` instances. Their tool descriptions, schemas and results are untrusted data. Semwright assigns their namespace and still applies normal broker policy, operator approval, cancellation, provenance and audit.

Application drivers use the same Provider Runtime. Driver Protocol semantics are shared; process launch, executable verification and isolation are platform responsibilities. On Linux, the Driver Host stages a digest-pinned ELF and requires bubblewrap plus Landlock. macOS driver/plugin execution remains fail-closed until a supported isolation model is proven; the portable Driver SDK does not weaken Linux to manufacture parity.

Recipes and plugins remain separate composition mechanisms: recipes re-enter broker execution for every step; plugins provide narrow one-shot commands.

## Execution and references

The broker snapshots the selected capability descriptor and provenance before dispatch. It evaluates capability/risk/scope, obtains the execution gate, requests human approval when required, validates current references, and invokes the selected provider with cancellation and deadline semantics. A provider failure does not trigger an implicit retry or hidden fallback.

References are opaque and session-scoped. Provider generations and backend fingerprints prevent known stale objects from silently becoming newly-created objects. Dynamic provider disconnects invalidate their catalog generation. Platform-native identifiers are implementation details, not agent authority.

Provider discovery is not authorization. Registering a driver or MCP upstream does not create its policy grant.

## Dependency direction

`types` owns transport-independent domain data. `registry` validates/indexes capability descriptors. `policy` owns authorization intent. `backend-api` owns Provider/Backend contracts. `core` owns provider leases, broker orchestration, refs and audit. `federation` implements MCP providers. `driver-sdk` is application-author facing and has no broker authority. `driver-host` adapts that protocol into a platform-specific sandboxed Provider.

The daemon is the composition root. It selects the compiled platform host and loads owner-configured providers. No root daemon, default TCP listener or automatic elevated helper is part of the architecture.

See [platforms](platforms.md), [compatibility](compatibility.md), [security](security.md), [VERIFY](../VERIFY.md) and [release blockers](../RELEASE_BLOCKERS.md).
