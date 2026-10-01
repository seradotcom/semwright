# Blender SDK / runtime gaps

These are transport/distribution or deliberate authority-boundary gaps; they do not silently expand the semantic-completeness claim.

## Driver Protocol

The current Blender manifest negotiates Driver Protocol v1. The SDK supports through v4, but some Blender native calls execute synchronously on Blender's main thread. Advertising cooperative cancellation for an in-flight native call would be misleading until the owned Blender process can be safely interrupted or recycled with truthful outcome reporting.

A future transport pass may add v3 progress/artifact/cancellation semantics behind a supervised process-restart boundary. Do not advertise those interfaces before that behavior exists.

## Distribution

The driver now requires two explicit owner authorities for Blender 4.5.14 LTS:

- a read-only `blender-runtime` directory mount containing the portable runtime resources and libraries; and
- a `blender-executable` tool grant for the exact `blender` file, verified by SHA-256 and materialized by Driver Host as `/plugin/tools/blender`.

The driver does not search `PATH`, `/usr/bin` or `/usr/local/bin`. CI downloads the official 4.5.14 LTS archive, verifies its published archive SHA-256, and then proves that the driver starts when the portable runtime is mounted only through these authorities.

The executable currently uses the existing direct sealed-tool materialization supported by Driver Host. Protocol v4 Host-mediated `ToolExecute` is not required because Blender is a persistent supervised child of the Blender driver rather than a short-lived per-request tool invocation.

A generic Semwright multi-tool runtime package format could make installation/distribution more ergonomic in the future, but it is no longer required to remove ambient executable discovery from this driver.

## Operator-only lifecycle

The generic semantic boundary never falls through to arbitrary `bpy.ops`. If the pinned Blender API exposes an authoring lifecycle only through context-sensitive operators and there is no dedicated context-safe wrapper, that lifecycle remains excluded until implemented as an explicit reviewed capability.

Examples of generic escape hatches that remain prohibited include arbitrary add-on execution, animation-driver expressions, Text/Python execution, unrestricted external-library operations and editor/UI operator surfaces.

The armature Edit Mode helper is a narrow reviewed exception: it uses fixed mode transitions internally to access Blender's edit-bone API; the agent cannot select an operator or arbitrary arguments.
