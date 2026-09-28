# Security delta — Composition / Motion / AV

This document describes the security surface added by the Composition/Media work. It does not close R16 and it is not an independent security review.

## Authority model

The common kernel is data and state-machine logic. It has no native application, filesystem, network, shell or plugin authority. Plans, profile descriptors, findings, Skills and artifact metadata do not grant permission.

Native effects still flow through the existing Broker → policy → provider/Driver Host path. Figma and Motion adapters bind server-issued plans to the Driver Host session and revalidate freshness before mutation. AV coordination is intentionally not a new privileged workflow engine: each native stage must be dispatched through the existing Broker execution channel.

A SHA-256 proves integrity, not authorization. PlanVault stores the complete canonical plan bytes, owner/session binding and cumulative budgets. Recomputed client digests do not authorize changed bytes or targets.

## New parsing and state surfaces

New bounded parsers/types include:

- canonical Composition JSON and strict duplicate-key decoding;
- rational media time, rates, cue graphs and time maps;
- high-level Film/sequence/beat/shot/subject authoring;
- temporal constraints and deterministic motion grammar;
- media artifact/provenance receipts;
- AV subplans, service proofs, sync probes and publication manifests.

Unknown fields, oversized collections, malformed digests, noncanonical rational values, nonfinite visual values, invalid graph references and unresolved required cues fail closed.

## Figma delta

The eight existing public Composition operations remain the public Figma surface. The adapter now requires the plan to have been issued to the current Host session and keeps the cumulative repair budget server-side.

Repair planning compares submitted deterministic findings against fresh native validation at the requested revision and against node identities observed inside the original Composition. Empty, forged, ambiguous, stale or unowned repair requests do not become authority. Budget overflow remains ResourceExhausted; policy denial remains distinct.

Figma still has best-effort revalidation rather than native compare-and-swap. Collaborative changes can therefore invalidate a plan between observation and native execution. The adapter does not claim distributed rollback or TOCTOU elimination.

## Motion Canvas delta

The high-level Film is typed data. Generated TypeScript imports a fixed first-party Motion Canvas implementation; Film fields cannot contain executable JavaScript, shell commands, package names, remote URLs, shaders or arbitrary signal names.

Authoring projects seal the derived managed model. External drift is detected before deterministic recompilation rather than silently overwritten.

Renderer instrumentation emits bounded NDJSON observations tied to the render input digest, source fingerprint and artifact manifest. Observed state is distinct from intent. Missing pixel/font/geometry evidence stays UNKNOWN.

The browser/runtime threat model remains the existing Motion Canvas Driver Host model: pinned tools, disposable browser state, network disabled, explicit grants and bounded process/output resources. The new authoring layer does not weaken those controls.

## AV and artifact delta

AV service proofs bind provider identity, generation, catalog digest, runtime digest and required stage descriptors. The fixed coordinator records prior effects and never retries a non-idempotent stage after a lost/unknown receipt.

Intermediate and final artifacts retain owner, source plan, source state, dependencies, digest, bytes, media type and observed stream metadata. Artifact references are tokens, not paths.

Final publication uses existing scoped Broker filesystem operations. A private content-addressed manifest is read back and then copied through artifact.handoff to an owner-configured destination. This is atomic pointer/file publication only; it does not make Figma, Motion, audio or MLT mutations a distributed transaction.

Final sync PASS requires decoded-media evidence against the originally declared cue/tolerance specification. Missing detections, sampled evidence when full scan is required, insufficient confidence or excessive uncertainty cannot be promoted to PASS.

## New denial/hostile cases that require evidence

The final candidate must exercise at least:

- client-recomputed changed plan bytes;
- cross-session plan/replay;
- stale document/project generation and provider generation;
- cumulative repair budget exhaustion;
- malformed/oversized authoring and cue graphs;
- derived-project drift;
- hostile asset/path fields and artifact-token/path confusion;
- incomplete native observations and zero-test selectors;
- lost mutation receipts and cancellation during a staged sequence;
- publication path traversal/self-overwrite and digest drift;
- final media with missing/ambiguous sync probes;
- policy denial/revocation between stages.

## R16 impact

Existing R16 evidence predates these parsers, lifecycle bindings, generated native authoring runtime and AV publication path. An independent review must re-cover:

1. Plan/session binding and replay behavior.
2. Figma fresh-validation repair authority.
3. Generated Motion code/data separation and asset/font handling.
4. Renderer observation provenance and output bounds.
5. AV stage/provider substitution and partial-effect handling.
6. Filesystem publication and artifact provenance.
7. Dependency/Skill supply-chain changes.

This mission leaves R16 open. Automated CI, fuzzing and sandbox tests are supporting evidence, not the independent review.
