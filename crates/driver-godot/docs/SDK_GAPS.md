# Godot Driver SDK findings and closeout status

The original isolated Godot pack was built against Driver Protocol v1 and correctly identified
several missing generic Host/SDK primitives. Those findings have now been closed in Semwright
rather than bypassed inside the Godot driver.

The production Godot driver uses **Driver Protocol v3**.

## Resolved — loopback-only bridge authority

Godot needs an authenticated local editor bridge, not ambient network access.

Driver Host now owns a configured `127.0.0.1` listener and proxies it to a private Unix socket
mounted inside the sandbox. The Godot driver remains in an isolated network namespace and can
run with `network=false` and owner network opt-in disabled.

The Host conformance test exercises this exact path.

## Resolved — owner secret delivery

Pairing material no longer needs to live inline in production driver configuration.

Driver manifests support first-class secret mounts. Driver Host requires the secret source to be
a canonical, small, regular owner-approved file and exposes only the named materialized secret to
the driver. The concrete path is platform-owned; Linux currently materializes it beneath
`/run/secrets/<name>`, while callers and production config use the logical secret name.

Godot production config uses `secret_name`; inline secrets require explicit development mode.
The historical `secret_file` form remains a Linux compatibility path, not the portable contract.

## Non-goal — caller authorization inside the driver

Broker/Policy remains the authority boundary. Drivers receive operations that have already been
authorized; they must not become a second policy engine based on caller identity.

Protocol v3 carries the Semwright session needed for ref scoping and execution correlation.
If future deployments require one driver process per tenant/agent, that is a Host lifecycle
isolation concern, not a requirement to give application drivers independent authorization
power.

## Resolved — broker-native provider-owned refs

Protocol v3 negotiates `native_refs`. Dynamic drivers can emit bounded `NativeTarget`
markers, Broker materializes them into opaque shared RefStore IDs, and the provider validates the
target before reuse.

Godot node/resource/scene refs retain project/session/generation/revision/fingerprint semantics
and fail closed on stale or changed targets.

## Resolved — persistent-driver CPU budgets

The original Linux `RLIMIT_CPU` remains a hard cumulative lifetime cap.

Driver resources additionally support an optional per-operation CPU budget on Linux and Windows.
On Linux, Driver Host accounts the live provider process tree. On Windows, the platform-owned Job
Object supplies monotonic cumulative user+kernel CPU for the full sandbox authority boundary,
including terminated descendants. Driver Host terminates the provider with a resource-exhausted
uncertain result when the operation budget is exceeded. Longer persistent lifetime caps are
permitted only when a bounded per-operation budget is configured.

Hosts without an equivalent bounded accounting primitive reject the opt-in per-operation budget.

## Resolved — secondary executable authority

Godot runner operations require an owner-approved Godot executable in addition to the driver
binary.

Driver manifests now support digest-pinned secondary tools. Protocol v5 lets the driver request
the logical `godot` tool through Driver Host with only the workspace mounts declared for that
tool. Linux and Windows both use Host-mediated invocation; the driver does not receive the owner
installation path. Linux v4 materialized-tool compatibility remains for older drivers.

Production Godot config likewise names project/output mounts rather than embedding
`/workspace/...`, Windows paths or a duplicate executable path/digest. Direct executable paths
remain available only in explicit development mode. The owner source may disappear after Host
staging; execution stays bound to the verified staged bytes.

## Resolved — companion EditorPlugin distribution

Driver Package v2 supports explicit companion payloads in addition to the driver executable.

Companion destinations are normalized relative paths with per-file SHA-256 and size metadata,
bounded file/count/aggregate budgets, duplicate rejection and exact payload accounting. Symlink
sources, traversal destinations, digest mismatches and undeclared trailing bytes are rejected.

For Godot, `companions.list` is checked against the reviewed
`integrations/godot/addons/semwright/` tree. Package installation stores companions in the
private installed-driver version directory. It does **not** copy or enable the plugin in a user
Godot project; activation remains an explicit owner action.

## Resolved by design — outer dry-run

The Broker's outer `dry_run` is not a missing Driver Protocol field.

Core handles outer dry-run before provider invocation and returns a non-side-effecting plan, so
an application driver cannot accidentally execute because it failed to receive a duplicated
runtime flag. Capability-specific Godot dry-run/preview fields remain semantic application
features, not security controls.

## Cross-platform certification

Linux x86_64 has real Godot editor acceptance plus bubblewrap/Landlock Driver Host evidence.

macOS, Windows and Linux ARM64 require their own real-editor and platform-host acceptance before
they are declared certified Godot targets. This is platform certification work, not an
unresolved P0/P1 Driver SDK blocker for the certified Linux path.

## Closeout

For the certified Linux target there are no unresolved P0/P1 Godot-specific Driver SDK gaps.

These generic improvements were intentionally implemented in Semwright Host/SDK instead of
granting Godot arbitrary GDScript, shell execution, unrestricted object calls, ambient network
access or unsandboxed fallback.
