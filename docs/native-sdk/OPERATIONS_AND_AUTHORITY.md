# Operations and authority

Discovery describes what a provider can do; it is not permission to do it. Every native operation has a canonical `CommandDescriptor` plus Native SDK operation guarantees, while Broker/Policy remains the authorization and approval authority.

## Execution boundary

`CLI/MCP -> daemon -> Broker/Policy -> Driver Host -> NativeDriver -> application`

`CallContext::application_local` is deliberately non-authoritative. Driver execution context is created by the canonical Host and cannot be reconstructed from operation JSON. The application cannot mint approval, filesystem grants, secrets, runtime tools or Broker refs.

Driver Host owns provider/session binding, native-ref resolution, grants, cancellation, sealed runtime tools, sandbox lifecycle and descendant cleanup. The application owns native identity/revision, transaction/commit point, domain validation and request deduplication when declared.

## Base and provider drift

Before a target-bound native operation, `NativeDriver` re-observes the resource and rejects stale application state. Core independently pins provider runtime/generation for rebuild preparation and rejects a preparation after provider refresh. The SDK does not replace either mechanism.

## Jobs

Long-running jobs use Core/Driver Host mechanisms. The Native SDK does not add a scheduler. Runtime-tool jobs are tested as detached, session-bound and cancellable, and uncertain application completion is not promoted to successful terminal state.
