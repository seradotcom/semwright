# Versioned subject compositing

The typed Motion authoring path supports Film version 2 for bounded subject-level
blend modes. Use `Subject.blend_mode` in `semwright-motion-authoring`. The native
projection is implemented by `semwright-driver-motion-canvas` and its generated
Motion Canvas 3.17.2 scene, not by an arbitrary script or a preview-only effect.

Before submitting v2, check the composition plan descriptor version (1.1) and
inspect the driver's reported `authoring_version` (2). The plan capability also
advertises the tag `film-v2-subject-blend-v1`. Use the established composition
plan/apply and render start/status/result protocol; no new write authority is
introduced. Do not downgrade a non-normal request to v1.

| Subject blend_mode | Native Canvas operation |
| --- | --- |
| normal (default) | source-over |
| multiply | multiply |
| screen | screen |
| add | lighter |

Normal is omitted from canonical serialized subjects, preserving the legacy
v1 JSON and its intent digest. Existing v1 scenes retain their native default
and traversal order. Rust clients constructing Subject directly must set the
new field (SubjectBlendMode::Normal for existing content). Non-normal v1 and
non-normal camera subjects are Unsupported before planning effects. Unknown
modes are rejected by typed deserialization. The compiler extension version is
2; regenerate compiled artifacts instead of treating v1 render receipts as v2
proof.

For v2, layer order is ascending among siblings; equal-order siblings retain
the authored array order, including when their parent is listed later. This
does not flatten or interleave separate parent subtrees. Fixed positions are
parent-local, with the existing Motion Canvas transform and clip semantics.
Non-normal subjects use the pinned Node composite operation, which composites
the cached subject subtree into its parent's active render target. Ancestor
cache boundaries isolate their contents; a child does not blend through such
a boundary into the global frame. Existing opacity and nested native clips
remain applicable. Add means the Canvas lighter operation: saturated addition
in premultiplied alpha, not unbounded HDR or a custom shader.

The source/projection tests distinguish wire compatibility, unsupported
profiles and native realization. The hosted subject-blend-only lane checks
the exact source, types, and the independently decoded real PNG produced via
Broker, Driver Host and pinned Firefox. Its four translucent overlap cases
also sample regions outside the inner and outer clips. The RGBA oracle computes
expected values independently of the renderer, allowing a small quantization
tolerance. The emitted pixel evidence, not the presence of a mode in the source,
is required to accept that rendered case. This does not certify arbitrary
combinations of camera, filters, media, or ancestor cache effects.

Pinned upstream implementation:
https://github.com/motion-canvas/motion-canvas/blob/v3.17.2/packages/2d/src/lib/components/Node.ts
