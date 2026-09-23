# Semantic video domain

Semwright has a backend-neutral video editing domain so agents can reason about a timeline
without inheriting the storage model of one editor or render engine.

The shared crate is `semwright-video-domain`. It is intentionally below concrete video drivers
and above their native project/runtime layers.

```text
             agent / recipe / broker
                      |
                      v
             semantic capabilities
                      |
                      v
            semwright-video-domain
       model / edit / time / refs / diff
                 / conformance
                      |
          +-----------+-----------+
          |                       |
          v                       v
     MLT adapter             future adapter
  native round trip        native API/project
          |
   MLT / Kdenlive /
       Shotcut
```

## Portable contract

Version 1 contains the common NLE concepts already demonstrated by the current backend:
`Project`, `Profile`, `MediaAsset`, `Sequence`, `Track`, `Timeline`, `Clip`,
`Transition`, `Effect`, `Keyframe`, `Marker`, `SubtitleReference`, rational
`FrameRate` and half-open `FrameRange`.

The shared edit engine currently covers project/profile, sequence creation, asset import/relink,
track lifecycle/state/order, clip insert/move/trim/split/remove/duplicate, transitions, effects,
keyframes, markers and audio volume/fades.

These are semantic operations. A concrete driver remains free to expose additional native
read-only information or application-specific capabilities.

## Backend capability snapshots

Every concrete video backend must classify all shared mutation operations explicitly through
`BackendCapabilities`. A snapshot is tied to `MODEL_VERSION` and contains one
`MutationSupport` value for every operation in `SEMANTIC_OPERATIONS`; missing and unknown
entries fail validation.

This is intentionally stricter than a sparse feature list. When the shared domain gains an
operation, existing backends must make an explicit compatibility decision instead of silently
appearing to support it. Backends may advertise only guarantees they actually enforce, such as
optimistic concurrency, differential semantic conformance and native round-trip validation.

The MLT backend derives this snapshot from its real project adapter on every project, so
Kdenlive/Shotcut/native-MLT graph restrictions remain visible without leaking those formats
into the shared domain.

## Native envelope rule

A backend may need much richer state to preserve a document exactly. That state stays in the
backend.

The MLT implementation, for example, keeps native format detection, original XML nodes,
bindings, proxy/original relationships and service metadata in `driver-mlt-video`. Its
`domain.rs` projection strips those details and maps opaque native structures to
`Editability::ReadOnly`.

This rule prevents a future backend from having to emulate another application's serialization
model merely to participate in semantic video editing.

## Differential conformance

The portable edit engine is executable specification, not documentation.

For each supported MLT mutation the driver now:

1. validates the native revision and backend support level;
2. projects the pre-edit project into the shared model;
3. translates the edit intent into the shared `Edit`;
4. performs its existing MLT-native semantic mutation;
5. projects the result;
6. calls `video_domain::conformance::verify_backend_mutation`;
7. only then performs its existing serialize/reparse validation and publication.

The gate compares the complete projected project plus affected IDs, created IDs and duration.
A mismatch fails closed with `BackendFailed`.

This means existing MLT tests exercise both implementations differentially. The shared crate
also has independent tests so its behavior does not depend on MLT.

## Support is not authorization

`MutationSupport` is shared vocabulary:

- `SafeRoundtrip`: backend can persist the semantic mutation safely;
- `MetadataRisk`: semantic mutation is possible but native metadata preservation has a known
  risk requiring explicit handling;
- `RenderOnly`: state can be represented/rendered but not safely mutated;
- `Unsupported`: backend cannot provide this mutation.

These states do not grant authority. Broker policy, risk classification, user consent and
driver sandboxing remain unchanged.

Backend wire compatibility may use a legacy/native spelling. For example, the MLT driver keeps
its existing `MLT_RENDER_ONLY` output string even though the shared in-process enum is
`MutationSupport::RenderOnly`.

## Rules for future backends

A future video backend should:

- depend inward on `semwright-video-domain`;
- keep native SDK/project types out of the shared crate;
- project unsupported or unknown structures conservatively;
- never turn loss of fidelity into an editable semantic approximation;
- use native revisions for optimistic concurrency when available;
- run the shared differential conformance gate before native publication;
- retain independent real-application/round-trip tests;
- extend the shared model only for concepts demonstrated to be cross-backend.

The second backend is the real test of every abstraction. If its native model exposes a concept
that cannot be represented without distortion, extend/version the domain rather than smuggling
backend-specific fields into generic types.

## Verification

The domain is included in normal Cargo workspace quality gates. The MLT driver's existing
round-trip, security, property, conformance and edit-engine suites remain in place; its native
live test still requires the external real MLT toolchain and sandbox prerequisites.

See also [architecture](architecture.md), [drivers](drivers.md) and the
[MLT driver](../crates/driver-mlt-video/README.md).
