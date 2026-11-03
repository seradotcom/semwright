# Linux runtime-tool stderr custody

Semwright's Linux Driver Host returns a sealed tool's bounded standard-error output to the initiating driver as part of the existing `ToolExecutionOutput` result. This is an execution diagnostic, not a new sandbox permission or a system-wide log.

## Isolation and limits

- The Host configures one private stderr pipe for a single launched tool. An independent drain handles writes during Bubblewrap startup as well as execution, without blocking the startup metadata handshake.
- Captured bytes remain scoped to that invocation. They are not written to public logs, persisted to custody files, or broadcast to other drivers.
- The existing output-size contract and transport-frame budget are enforced independently. Overflows produce an explicit bounded error rather than silently truncating output or publishing partial data.
- The normal sandbox spawn path continues discarding stderr unless its owner explicitly selects private observation.
- A read failure, incomplete capture, timeout or cancellation does not justify declaring that an already-started command had no effect. Such failures retain an uncertain outcome so consumers must inspect real state before attempting a new invocation.
- Detached tool jobs are not consumed until their terminal reply has passed the encoded frame limit. Repeated collection of a completed job is rejected.

## Verification

The native Host workflow exercises genuine Bubblewrap/Landlock tool launches, separated stdout/stderr, a nonzero exit status, exact-size output and overflow handling, cancellation, deadline handling, and the preservation of uncertain side effects. The fixture uses disposable directories and no private application media.

This change does not add automatic retries, expand the network or filesystem grants, or change driver manifests. It is independent of the daemon's separate startup diagnostics.
