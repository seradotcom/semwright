# Authoring and evidence

The high-level Film model contains sequences, beats, shots, subjects, layers, annotations, captions, temporal constraints, cues, editorial tokens and output profiles. Narrative fields declare intent; they are not automatic creative judgment.

Motion primitives lower to a closed native operation set. Use the capability schemas for parameters and compatibility. Morphing requires compatible path topology. Shared-element and carry-forward relations require resolvable logical subjects. Overlapping writes to one native channel are rejected.

Temporal schedules use exact rational time until a declared frame boundary. Hard constraints outrank preferred durations. An unresolved cue is UNKNOWN, not time zero. A duration that is not an exact frame boundary for the selected rate must be resolved explicitly.

Renderer observations are separate from authoring state. Native readback can cover transformed bounds, local size, draw state, opacity, z-order, clipping, text/font readiness, cue state and transition completion. Pixel visibility can remain UNKNOWN even when an object was drawn with nonzero opacity.

A full-Film PASS requires the requested range and every required rule to have sufficient evidence. A sampled range cannot certify absence of defects outside the sample.
