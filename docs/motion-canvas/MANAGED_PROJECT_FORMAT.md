# Managed Motion Canvas project format

Format version: **1**. Component library version: **1**. Authoritative filename: `semwright-motion.json`.

A managed project contains a project identity/generation/revision, render settings, theme, bounded project variables, ordered scenes, local assets and audio tracks. Scenes contain a hierarchical node tree plus declarative animations, cues and an optional transition.

## Identity and time

IDs are bounded canonical identifiers and are distinct from display names. A project generation is a 32-hex token. Revisions are positive monotonically advancing integers for semantic changes.

Time is stored as integer milliseconds. Frame conversion uses integer arithmetic and round-half-up at an integer FPS; the format does not invent fractional Motion Canvas frame-rate support. Scene/project duration and render-frame counts are bounded.

## Nodes and properties

Version 1 node kinds are: group, layout, rect, circle, line, text, code, svg, image, video, latex, camera, grid, polygon, path, cubic_bezier, quad_bezier, spline, knot and ray. Properties have two bounded representations: strongly typed first-class fields for the original managed contract and a version-pinned `semantic` map for additional upstream 3.17.2 properties. Both routes resolve through the same registry; unknown properties remain invalid. The registry reports exact upstream property name, value kind, storage mode, animatability, enum values and source Props interface.

Node filters are a bounded list of invert/sepia/grayscale/brightness/contrast/saturate/hue/blur values and compile through the official Motion Canvas helpers. Spline can use fixed points or managed Knot children. Path data stays a bounded string value; no callback/spawner/shader source can enter the semantic property map. Fill/stroke can use canonical bounded colors or a declarative linear/conic/radial Gradient with ordered bounded stops; live `Pattern`/`CanvasImageSource` identity is intentionally not serializable.

Layout preserves Motion Canvas value unions rather than coercing them to pixels: width/height and row/column gaps accept numeric or percentage lengths; min/max limits and flex basis retain content keywords; direction includes reverse modes; alignment includes baseline/space-evenly; text wrapping includes `pre`; and the layout mode can inherit, explicitly enable or explicitly disable. The historical structured layout bundle remains backward compatible while `layout_mode`, `offset` and `line_height_value` expose exact canonical upstream semantics. Corner radius accepts canonical one/two/three/four-value spacing. Diagram edges reference endpoint node IDs in the same parent coordinate space.

Code contents are bounded display data with a fixed language/highlighter allowlist. Selection supports line, word and arbitrary bounded CodeRange data, while dynamic CodeScope/CodeTag/SignalValue construction remains excluded. SVG is either an inline sanitized non-active structural subset or a managed local asset. Remote URLs are not an asset source. LaTeX accepts bounded `string` or segmented `string[]` source within the managed command vocabulary; the driver never shells out to TeX.

## Project variables

`variables` is a bounded map of canonical identifiers to primitive semantic values (boolean, finite number, bounded text, vec2, spacing or bounded number list). Variable set/remove is transactional, appears in semantic diff and compiles to `makeProject({variables: ...})`. Arbitrary objects/functions are deliberately not accepted.

## Animations, cues and components

Animations name a target, allowlisted property, typed from/to values, integer timing anchor/duration and curated easing. In addition to the original named animation properties, `semantic(name)` can animate only registry properties whose bounded value type has a safe Motion Canvas signal interpolation. The fixed easing enum covers the standard non-parameterized Motion Canvas timing functions; scene transitions include fade, four slide directions, zoom-in and zoom-out. Overlapping writes to the same effective property are rejected unless represented through supported grouping semantics.

Cues have stable IDs, unique names per scene, integer start and duration. Animations may anchor to cues or use cue duration.

Safe component presets compile to ordinary model nodes: Title, Subtitle, Badge, Panel, TerminalWindow, CodePanel, ArchitectureNode, ArchitectureEdge, CapabilityChip, MetricCounter, BrowserFrame, AppCard, Callout and LogoLockup. Component version is stored in the project.

## Refs and stale detection

Refs encode project ID, generation, revision, semantic-source SHA-256, object kind and stable ID. Project/scene/node/asset/cue/animation/render-job kinds are distinct. A changed source fingerprint, generation, revision, kind or removed object invalidates an old ref.

Display names never establish identity. Recreating an object with the same display name does not resurrect its old ref.

## Persistence and generated output

The semantic file is persisted with temp-file, validation, fsync, source recheck, atomic rename and directory fsync. The source is preserved on failure.

Generated source is stored in a content-addressed `.semwright-generated-<sha256>` tree under the granted project root. It includes scene TSX, project metadata, fixed exporter integration, TypeScript/Vite config and exact package lock. Same model + compiler/runtime version produces the same generated file inventory. Semantic-completeness code generation is compiler version 2 while the persisted managed-format schema remains version 1 and backward compatible through defaulted fields.

Assets are copied only from validated project-relative paths, bounded in size and checked against the semantic SHA-256/byte length. The generated tree is verified before reuse.

## External project boundary

The read-only `project.detect` capability inspects bounded package metadata and the conventional project entry without loading or executing user TypeScript. An observed Motion Canvas dependency is reported as evidence; the driver never installs it.

A valid `semwright-motion.json` selects managed mode. Otherwise external projects remain non-mutating: adoption, arbitrary TSX round-trip editing and package installation are outside format v1.
