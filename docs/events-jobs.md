# Events and jobs

Semwright has one broker-owned event stream and a bounded, session-scoped job model.
Neither grants authority by itself: policy remains attached to the capability that produced
an event or to the nested request executed by a job.

## Event provenance

Events carry typed source/provider provenance without changing the daemon's sequence/replay
transport. Provider-originated payloads remain explicitly untrusted data, and reserved
provenance fields cannot be smuggled through provider attributes.

Replay and live delivery honor an optional broker-session audience. A session cannot use
the subscription stream to observe private job lifecycle events owned by another session.
Provider disconnect and capability-refresh events use the same broker-bound provenance.

Driver Protocol v2 can transport bounded child events. A driver must negotiate the events
interface explicitly; protocol v1 remains fail-closed for unsolicited child events.

## Job control

The built-in commands are:

```text
jobs.start
jobs.get
jobs.list
jobs.cancel
```
`jobs.start` accepts a normal Semwright execute request as its nested request. The broker
validates that request before reserving a job, then executes it by re-entering the normal
broker path. The nested capability therefore receives its own schema validation, policy,
confirmation, timeout, provider provenance and audit decision.

`jobs.list` is session-scoped and returns retained jobs newest first. `jobs.get` and
`jobs.cancel` require ownership by the same broker session. Session revocation cancels
and forgets only that session's active jobs.

Jobs move through:

```text
queued -> running -> succeeded | failed | cancelled
```

Cancellation is idempotent. Provider execution uses the same cancellation token, and
Driver Protocol v2 adds a negotiated cooperative-cancellation acknowledgement for driver
children. Observation and cancellation remain outside the provider execution gate so an
operator can stop a blocked long-running operation.

## Progress and artifacts

Provider Runtime accepts progress only from the provider currently executing the correlated
job request. `JobProgress` contains a completed value, optional total and optional bounded
message. Providers must report measured state; Semwright does not manufacture percentages.
A progress update may carry at most 32 validated `JobArtifact` entries. Artifact metadata
is bounded and retained by reference; repeated references replace prior metadata rather
than growing the job indefinitely. Progress/artifact state is visible through `jobs.get`,
`jobs.list`, lifecycle events and the read-only inspector.

Retained state remains bounded to 256 jobs broker-wide, 64 retained jobs per session,
16 active jobs per session and 256 KiB maximum retained result envelope per job. Oversized
terminal result bodies are omitted while lifecycle state remains available.

## MCP Tasks

The MCP frontend maps Semwright jobs to the negotiated `io.modelcontextprotocol/tasks`
extension through the official Rust SDK. `semwright_execute_task` is available only when
the client negotiated Tasks. Task IDs are the session-scoped JobStore IDs; task get/result
and cancellation re-enter normal broker policy. Legacy clients cannot create tasks.

Jobs are in-memory and are not durable across daemon restarts. Semwright currently has no
`input_required` job transition, so `tasks/update` does not fabricate one.

## Verification

Hosted tests cover policy re-entry, cross-session privacy, revocation, idempotent
cancellation, lifecycle events, blocked-provider cancellation, provider progress/artifact
correlation, MCP Tasks create/poll/result/cancel, and a sandboxed Driver Protocol v2 fixture
that exercises events, dynamic capability invalidation, progress/artifacts and cooperative
cancellation. Protocol v1 compatibility remains supported without advertising v2-only
interfaces.
