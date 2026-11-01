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
- A real Windows sandboxed conformance driver was terminated deliberately through its protocol-v2 `driver.fixture.disconnect` capability. The still-running broker republished a live provider generation (160 → 162), and a new read-only `driver.fixture.ping` succeeded: **PASS**. The failing request returned `Unavailable`; the broker was never restarted. No unrelated process was terminated.
- The executable has not been installed or rolled out globally. Longer-duration fault monitoring and real application state restoration remain outside this conformance check; recovering a driver process does not reconstruct unsaved application state.

### Reproduce the physical Windows check

Build the binaries from the same revision, and supply a separate trusted directory for disposable test state:

```powershell
cargo build --locked -p semwright-daemon --bin semwrightd -p semwright-plugin-host --bin semwright-sandbox -p semwright-driver-host --bin semwright-driver-fixture -p semwright-cli --bin semwright
python scripts/verify-driver-recovery-windows.py --bin-dir target/debug --cli target/debug/semwright.exe --work-root .local
```

The script only launches its own instance under an owner-only test directory. It verifies a pinned sandboxed driver, a controlled child exit, a new generation and a subsequent successful read. It never changes installed broker files or retries the failed command. Results and bounded diagnostic logs remain in the protected test directory.
