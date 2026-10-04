# Project Graph and Effects

The Native SDK adapts into canonical Project Graph and Effect Conformance authorities; it does not clone either authority.

## Project Graph

Feature `graph` constructs canonical `RevisionCandidate` and `DurableLocator` values. Opaque application revisions are digested as exact token tuples. The SDK adapter cannot admit evidence: a trusted Graph owner must authenticate the provider session and use canonical `RevisionAdapter`.

Native conformance explicitly checks changed projection => `STALE`, equivalent independently evidenced projection => `CURRENT`, and incomplete coverage => `UNKNOWN`.

The real Host Scene E2E exports through Driver Host, performs Broker `artifact.handoff`, registers the admitted immutable bytes in the canonical private Project Graph, and reads provenance through the same daemon/Broker session. The admitted destination is not mounted back into the native application.

## Effects

Feature `effects` reuses `semwright-effect-conformance`. The Host path is native operation -> export -> Broker artifact handoff -> immutable admitted bytes -> protected readback -> canonical evaluator -> operation-bound report.

A wrong expected artifact hash is rejected before admission. Separate conformance covers wrong values, substituted plan/runtime/source digests, partial scope and UNKNOWN/independent findings. The SDK does not mint a PASS verdict itself.
