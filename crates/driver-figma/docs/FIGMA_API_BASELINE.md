# Figma Plugin API baseline — 2026-09-23

Pinned typings: `@figma/plugin-typings 1.138.0` (npm latest observed at implementation time). Plugin manifest API remains `1.0.0`.

New plugins require `documentAccess: "dynamic-page"`. This pack uses async node/page APIs and page loading rather than assuming the whole document is resident.

The official manifest supports scheme-qualified network allowlists and `devAllowedDomains`; the development plugin restricts its bridge to `ws://127.0.0.1:38471` and has no external domains.

Editors recognized by current typings include Figma Design and FigJam plus newer editor-specific node types. This pack explicitly gates FigJam operations and treats unimplemented editor surfaces as unsupported.

Motion was introduced in Plugin API Update 130 (June 2026) and remains explicitly **Beta** in current official documentation. Current APIs include `figma.motion.figmaAnimationStyles`, `physicalSpringToNormalized`, node animation styles, manual keyframe tracks, timelines and timeline duration. Animated export overloads include MP4, GIF and WebM on supported nodes.

Prototype reactions are read through `reactions`; with dynamic-page access mutation uses `setReactionsAsync`.

Variables, local collections, component/instance APIs, FigJam creation APIs, CSS readout where exposed, and standard PNG/JPG/SVG/PDF export are represented in the plugin surface. Shader/Slides support is capability-gated future work, not claimed here.
