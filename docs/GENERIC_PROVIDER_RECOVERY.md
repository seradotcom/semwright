# Generic provider recovery

Semwright supports bounded, opt-in recovery for **any owner-configured Driver Provider**, independent of the application being automated.

## Enable

In the broker's owner-controlled TOML configuration:

```toml
driver_recovery = true
drivers = ["/absolute/path/to/verified-driver-manifest.json"]
```

The default is `false`. Existing policies, sandbox grants, executable digests, resource limits and network permissions are **not** changed. Configuration is consumed only at daemon startup; it does not grant any new capabilities.

## Contract

- An unexpected child-process exit is observed through the provider's existing close token. The current generation becomes unavailable and in-flight operations are cancelled.
- The supervisor makes **at most three restart attempts during a broker lifetime**, after 1, 3 and 9 seconds respectively. It never polls a healthy child or enters an infinite crash loop.
- Every restart stages and verifies the original owner-pinned executable, checks the same manifest identity and resource policy, performs the sandbox/Host handshake, and validates the capability catalog.
- Publication of the new generation is atomic and rejects competing replacements, manual removal, mismatched identities, incompatible descriptors and live originals. Previous native references are invalidated.
- A successful restart emits `provider.reconnected`. Consumers must rediscover capabilities and acquire fresh references. A prior session ticket can be retained if the broker remained alive; restarting the broker itself requires a fresh handshake.
- No pending or uncertain request is replayed. In particular, mutations, approvals, external side effects and writes must be reconciled against real state by the caller before any new invocation.
- If the attempt budget is exhausted or a policy/identity error occurs, the provider remains unavailable until the owner intervenes. The daemon does not silently change grants or fall back to an unsandboxed process.

The MCP frontend also recovers from a rejected **stale session handshake** (e.g. a broker restart) without retrying the previous tool call.

## Scope and limits

Recovery is implemented at the shared broker/Driver Provider boundary, not in any application-specific adapter. It does not restore an application's unsaved documents, revive obsolete native handles, promise exactly-once execution or recover every external MCP upstream. Operators can leave `driver_recovery = false` for workflows requiring manual process lifecycle control.

## Verified on Windows (2026-10-31)

- Full broker provider-runtime regression: **23 PASS / 0 FAIL**, including recovery, reference invalidation, cancellation and removal.
- Owner configuration opt-in and bounded restart policy: focused tests **PASS**.
- Windows ticket persistence: **2 PASS / 0 FAIL**; owner-only files use synchronous Win32 byte-range locking compatible with the declared Rust 1.88 MSRV.
- `cargo check` for the daemon and MCP frontend: **PASS**. Strict Clippy on the affected packages: **PASS**, with a pre-existing unrelated lint exception.
- The native MCP frontend retained its process across a deliberate restart of an isolated broker, with successful read-only calls both before and afterward: **PASS**. Only that temporary broker was stopped.
- The executable has not been installed or rolled out globally. A physical crash/restart of a real sandboxed application driver, and longer-duration fault monitoring, still require release acceptance; these tests do not establish that every driver preserves its application state.
