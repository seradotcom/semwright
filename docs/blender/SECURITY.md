# Blender semantic driver security

The Blender semantic driver owns a private `--background --factory-startup --disable-autoexec`
process inside DriverProvider's Bubblewrap + Landlock sandbox. Network access remains disabled and
the only writable application mount is the owner-granted workspace.

## Semantic authority

RNA metadata is discovery data, not permission. Generic mutation is restricted to bounded
boolean/integer/finite-float/string/enum values and short arrays. Pointer/collection traversal
uses RNA-declared property identity, bounded path depth and revision-bound refs.

The generic substrate refuses filesystem path subtypes, executable/script relations, animation
driver relations, runtime back-references and other values outside the bounded codec.
Dedicated capabilities are used where safe authoring requires lifecycle methods rather than simple
property assignment: datablocks, links, modifiers, constraints, keyframes, nodes, topology,
attributes, UVs, vertex groups, shape keys, splines and armature bones.

Asset loading accepts only clean workspace-relative paths, allowlisted extensions and single-link
regular files below the semantic size ceiling. It resolves the final path under the canonical
workspace before calling Blender.

## Explicit non-capabilities

There is no generic Python execution, generic operator invocation, shell command, arbitrary
external debugger/listener or network URL. Script nodes, Text/code surfaces and animation-driver
expressions remain outside the managed boundary.

Armature creation is the narrow exception that uses a fixed internal `bpy.ops.object.mode_set`
transaction because Blender's edit-bone API is only valid in Edit Mode. The operation name,
arguments and mode transitions are hard-coded by the driver; agents cannot select an operator.
