# Blender SDK / runtime gaps

These gaps do not change the semantic-completeness boundary for persistent authoring data.

## Driver Protocol

The current Blender manifest still negotiates Driver Protocol v1. The SDK supports v3, but the
Blender bridge executes some native `bpy` calls synchronously on Blender's main thread. Advertising
cooperative cancellation for an in-flight render/save/native operation would therefore be
misleading unless the driver can safely abort or recycle the owned Blender process.

A future transport pass may add protocol-v3 progress/artifact/cancellation semantics with a
supervised process restart boundary. Do not mark those interfaces true before that behavior exists.

## Distribution

The driver expects a pinned Blender runtime to be installed by the owner/distribution layer. CI
pins Blender 4.5.14 LTS by URL and SHA-256. A generic Semwright multi-tool runtime package format
would make installation cleaner but is not required for semantic correctness.

## Excluded authority domains

External .blend library linking, arbitrary add-ons, Text/Python execution, generic operators and
animation-driver expressions require separately reviewed capabilities. They should not be added as
generic RNA escape hatches.
