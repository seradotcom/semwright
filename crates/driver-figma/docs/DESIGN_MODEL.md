# Semantic design model

Rust types cover document/node refs, geometry, layout, paints, prototype reactions, Motion timelines/tracks/keyframes and validation findings. Refs are scoped by plugin session, document identity, generation, revision, node ID/type and fingerprint.

Unknown upstream node/effect payloads must remain opaque data rather than crash the driver. Large tree/search output is bounded. Child z-order remains ordered in canonical snapshots.
