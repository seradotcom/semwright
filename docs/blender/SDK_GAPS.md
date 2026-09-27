# Blender SDK / runtime gaps

These are transport/distribution or deliberate authority-boundary gaps; they do not silently expand the semantic-completeness claim.

## Driver Protocol

The current Blender manifest negotiates Driver Protocol v1. The SDK supports v3, but some Blender native calls execute synchronously on Blender's main thread. Advertising cooperative cancellation for an in-flight native call would be misleading until the owned Blender process can be safely interrupted or recycled with truthful outcome reporting.

A future transport pass may add v3 progress/artifact/cancellation semantics behind a supervised process-restart boundary. Do not advertise those interfaces before that behavior exists.

## Distribution

The driver expects the owner/distribution layer to provide a pinned Blender runtime. CI pins Blender 4.5.14 LTS by URL and SHA-256. A generic Semwright multi-tool runtime package format would simplify installation but is not required for semantic correctness.

## Operator-only lifecycle

The generic semantic boundary never falls through to arbitrary `bpy.ops`. If the pinned Blender API exposes an authoring lifecycle only through context-sensitive operators and there is no dedicated context-safe wrapper, that lifecycle remains excluded until implemented as an explicit reviewed capability.

Examples of generic escape hatches that remain prohibited include arbitrary add-on execution, animation-driver expressions, Text/Python execution, unrestricted external-library operations and editor/UI operator surfaces.

The armature Edit Mode helper is a narrow reviewed exception: it uses fixed mode transitions internally to access Blender's edit-bone API; the agent cannot select an operator or arbitrary arguments.
