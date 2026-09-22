# Events and jobs

Semwright has one broker-owned event stream and a bounded, session-scoped job model.
Neither mechanism grants authority by itself: policy remains attached to the capability
that produced an event or to the nested request executed by a job.

## Event provenance

Events carry typed source/provider provenance without changing the daemon's existing
sequence/replay transport. Provider-originated payloads remain explicitly untrusted data.
Reserved provenance fields cannot be smuggled through provider attributes.

Replay and live delivery honor an optional broker session audience. A session cannot use
the subscription stream to observe job lifecycle events owned by another session.

Provider disconnect and capability-refresh events use the same broker-bound provenance
rather than trusting source identity claimed by a provider payload.

## Job control

The built-in commands are:

```text
jobs.start
jobs.get
jobs.cancel
```

`jobs.start` accepts a normal Semwright execute request as its nested request. The brokervalidates that request before reserving a job, then executes it by re-entering the normal
broker path. The nested capability therefore receives its own schema validation, policy,
confirmation, timeout, provider provenance and audit decision.

The job-control command itself cannot turn an observe-only session into mutation authority.

Jobs move through:

```text
queued -> running -> succeeded | failed | cancelled
```

Cancellation is cooperative through the same cancellation token used by provider execution.
Repeated cancellation is idempotent. Cancellation and observation are deliberately outside
the provider execution gate so an operator can stop a blocked long-running operation.

## Scope and bounds

Jobs are private to the broker session that created them. Session revocation cancels and
forgets that session's jobs without exposing or changing jobs owned by another session.

The current development implementation bounds retained state to:

- 256 jobs broker-wide;
- 64 retained jobs per session;
- 16 active jobs per session;
- 256 KiB maximum retained result envelope per job.

When a completed result exceeds the retention budget, lifecycle state remains available but
the result body is omitted rather than retained unboundedly.

Job lifecycle events include queued, started, cancellation-requested and terminal states.
Those events are audience-scoped to the owning session.

## Deliberate limits

This development line does **not** yet define a universal progress percentage, driver artifact
model, remote task persistence, or automatic MCP Task mapping. Providers must not invent
progress that they cannot measure.

Driver protocol v1 also does not yet negotiate dynamic jobs/events as a provider interface.
Those follow-on contracts remain release blockers and must be added with compatibility and
conformance tests rather than inferred from this core job store.

## Verification

The workspace integration tests cover read-only completion, policy denial through a job,
cross-session privacy, session revocation, idempotent cancellation, lifecycle events, and
cancellation of a blocked dynamic provider. Hosted x86_64/ARM64 quality gates, fuzz, coverage,
real Chromium and driver conformance must be green on the exact commit before this document
is treated as accepted development evidence.
