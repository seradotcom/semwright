# Managed Motion Canvas project format

Format version: **1**. Component library version: **1**. Authoritative filename: `semwright-motion.json`.

A managed project contains a project identity/generation/revision, render settings, theme, ordered scenes, local assets and audio tracks. Scenes contain a hierarchical node tree plus declarative animations, cues and an optional transition.

## Identity and time

IDs are bounded canonical identifiers and are distinct from display names. A project generation is a 32-hex token. Revisions are positive monotonically advancing integers for semantic changes.

Time is stored as integer milliseconds. Frame conversion uses integer arithmetic and round-half-up at an integer FPS; the format does not invent fractional Motion Canvas frame-rate support. Scene/project duration and render-frame counts are bounded.

## Nodes and properties

Version 1 node kinds are: group, layout, rect, circle, line, text, code, svg, image, video, latex and camera. Properties are an allowlisted typed structure. Validation further restricts each property by node kind, numeric bounds, text/code size, path rules and asset type.

Layout is structured row/column layout with gap, four-sided padding, alignment, justification, grow and optional basis. Diagram edges reference endpoint node IDs in the same parent coordinate space. Code contents are display data only and use a bounded language enum.

SVG is either an inline sanitized subset or a managed local asset. Remote URLs are not an asset source. LaTeX uses a bounded vocabulary; the driver never shells out to TeX.

## Animations, cues and components

Animations name a target, allowlisted property, typed from/to values, integer timing anchor/duration and curated easing. Overlapping writes to the same effective property are rejected unless represented through supported grouping semantics.

Cues have stable IDs, unique names per scene, integer start and duration. Animations may anchor to cues or use cue duration.

Safe component presets compile to ordinary model nodes: Title, Subtitle, Badge, Panel, TerminalWindow, CodePanel, ArchitectureNode, ArchitectureEdge, CapabilityChip, MetricCounter, BrowserFrame, AppCard, Callout and LogoLockup. Component version is stored in the project.

## Refs and stale detection

Refs encode project ID, generation, revision, semantic-source SHA-256, object kind and stable ID. Project/scene/node/asset/cue/animation/render-job kinds are distinct. A changed source fingerprint, generation, revision, kind or removed object invalidates an old ref.

Display names never establish identity. Recreating an object with the same display name does not resurrect its old ref.

## Persistence and generated output

The semantic file is persisted with temp-file, validation, fsync, source recheck, atomic rename and directory fsync. The source is preserved on failure.

Generated source is stored in a content-addressed `.semwright-generated-<sha256>` tree under the granted project root. It includes scene TSX, project metadata, fixed exporter integration, TypeScript/Vite config and exact package lock. Same model + compiler/runtime version produces the same generated file inventory.

Assets are copied only from validated project-relative paths, bounded in size and checked against the semantic SHA-256/byte length. The generated tree is verified before reuse.

## External project boundary

The read-only `project.detect` capability inspects bounded package metadata and the conventional project entry without loading or executing user TypeScript. An observed Motion Canvas dependency is reported as evidence; the driver never installs it.

A valid `semwright-motion.json` selects managed mode. Otherwise external projects remain non-mutating: adoption, arbitrary TSX round-trip editing and package installation are outside format v1.
