# Semantic design model

Rust types cover document/node refs, geometry, layout, paints, prototype reactions, Motion timelines/tracks/keyframes and validation findings. Refs are scoped by plugin session, document identity, generation, revision, node ID/type and fingerprint.

Unknown upstream node/effect payloads must remain opaque data rather than crash the driver. Large tree/search output is bounded. Child z-order remains ordered in canonical snapshots.

High-level authoring adds `FigmaCompositionSpecV1`, `FigmaPlanV1` and `FigmaChangeSetV1`. The composition graph is represented as a bounded flat logical-ID table so cycles/depth can be rejected before execution. Plans bind document/session/generation/revision and a SHA-256 digest. See [COMPOSITION_MODEL.md](COMPOSITION_MODEL.md) and [CHANGESETS.md](CHANGESETS.md).
