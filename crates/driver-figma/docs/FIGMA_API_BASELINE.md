# Figma API baseline — 2026-09-23

Pinned Plugin API typings: `@figma/plugin-typings 1.139.0`. Plugin manifest API remains `1.0.0`.

The semantic API compiler inventories the pinned typings and currently records 18 global interfaces, 14 auxiliary interfaces, 34 scene-node types, 213 global members, 49 auxiliary method entries and 3,699 scene-node members. The checked-in coverage file reports zero unclassified public method names.

Update 139 adds composed color variables (`VariableComposedColor`) and the `COLOR_OPACITY` scope. Earlier 2026 updates add variable fonts, EASING/TIMING variables, Motion playhead/timelines/keyframes, animated MP4/GIF/WebM export and shaders. These surfaces are represented by the semantic driver rather than reached through arbitrary JavaScript.

Dynamic-page document access is assumed. Node/page access uses current async APIs and editor-specific operations are gated by the active editor and/or dedicated manifest.

The optional REST transport is pinned to `figma/rest-api-spec` commit `04fbbc719706e986fc79f3050d3e068e118275d9` and maps all 54 operationIds in that OpenAPI snapshot. Deprecated legacy project endpoints remain explicitly labeled while current folder endpoints are preferred.

Coverage is an exhaustiveness contract for the pinned public APIs, not evidence that every route has been exercised against a real customer file. Real-Figma acceptance remains a separate protected/manual gate.
