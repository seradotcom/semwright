# Semwright semantic video domain

This crate is the backend-neutral semantic core for non-linear video editing in Semwright.

It models the concepts an agent should reason about:

- projects and profiles;
- assets and resources;
- sequences, tracks and lanes;
- clips and half-open source/timeline ranges;
- transitions;
- effects and keyframes;
- markers and subtitles;
- opaque semantic references;
- mutation support and editability;
- transactional edit intent, diffs and deterministic identities.

It deliberately does **not** model a concrete editor's project file, executable, IPC
transport, render process, filesystem confinement or application metadata.

## Dependency direction

```text
agent / broker / driver
          |
          v
semwright-video-domain
 model / time / refs / edit / conformance
          ^
          |
   backend projection
          |
   concrete video driver
```

Concrete drivers own the native representation and translate it into this model. They may keep
arbitrary native round-trip state in a separate envelope, but that state must never become part
of the portable semantic contract.

The current MLT driver is the first conforming backend. Its native model still preserves the
MLT/Kdenlive/Shotcut graph needed for lossless and conservative round trips, while
`driver-mlt-video/src/domain.rs` projects only semantic state into this crate.

## Mutation contract

The edit engine applies a bounded `Edit` to a clone and returns an `EditOutcome`. It does not
authorize the operation and it does not decide whether a backend can safely persist it.

A concrete backend must perform, in order:

1. native optimistic-concurrency/revision validation;
2. backend-specific support and policy precondition checks;
3. projection of the current native project into `video-domain::Project`;
4. execution of the backend-native mutation;
5. projection of the resulting native state;
6. `conformance::verify_backend_mutation` against the shared edit engine;
7. native serialize/reopen or equivalent round-trip validation;
8. atomic publication through the driver's normal security boundary.

A mismatch in step 6 is a backend failure. It must not be hidden by changing the shared model or
silently accepting a backend-specific semantic result.

## What stays backend-specific

The following do not belong in this crate:

- native project formats and version detection;
- native object IDs or serialization nodes;
- proxy/original media metadata;
- application-specific service/plugin identifiers unless mapped to a semantic effect;
- render binaries and subprocess supervision;
- filesystem grants and source/output roots;
- application installation/discovery;
- GUI reopen certification;
- native revision calculation;
- backend-specific support strings exposed for compatibility.

For example, a backend may internally call a volume effect by an application-specific service
name. The portable domain sees `Effect { kind: "volume", parameter: "level", ... }`.

## Editability and support

`Editability` describes whether an observed semantic object may be edited through the shared
engine. Unknown native structures should project as `ReadOnly` instead of being flattened into
something that looks safely editable.

`MutationSupport` describes a backend's ability to persist a semantic operation:

- `SafeRoundtrip`
- `MetadataRisk`
- `RenderOnly`
- `Unsupported`

This is capability information, **not authority**. Semwright policy, consent and the Driver Host
remain responsible for authorization.

Every backend must also expose a complete capability snapshot for all shared mutation operation IDs.
Sparse maps are rejected, so adding a new shared operation forces every backend to classify it explicitly.

## Identity and revisions

The domain uses stable semantic IDs and opaque process-local refs. `apply_with_identity_base`
allows a concrete backend to derive newly-created semantic IDs from its own native optimistic
revision, so the portable engine and backend produce the same identities during differential
checking.

The shared semantic digest is deterministic for a given normalized project. It is not a
replacement for the native file revision when a backend has a stronger revision source.

## Time model

All edit math is frame-based and exact:

- timeline and source intervals are half-open: `[start, end)`;
- frame rates are rational;
- no floating-point time arithmetic is used;
- drop-frame timecode is explicitly supported for the curated rates;
- conversions are bounded.

Adapters convert any native inclusive/out representation at their boundary.

## Adding another video backend

A new video driver should not fork this model. It should:

1. define its native project/envelope privately;
2. implement a total, conservative semantic projection;
3. map only genuinely supported operations into `Edit`;
4. mark unrepresentable native structures read-only;
5. reuse `MutationSupport`, time primitives and refs where appropriate;
6. run the shared differential conformance gate for every mutation;
7. maintain its own real-application round-trip and render tests.

If a second backend demonstrates that the semantic model is missing a real cross-editor concept,
extend and version the shared model with fixtures and differential tests. Do not add speculative
fields for a single backend.
