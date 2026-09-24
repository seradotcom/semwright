# Compatibility

| Surface | Status |
|---|---|
| Semwright Driver Protocol | v2; child events negotiated and exercised |
| Figma Design | Broad typed semantic surface; public Plugin API inventory classified; real-Figma acceptance pending |
| FigJam | Tables, timer, diagram primitives, active-user gated surfaces and editor-specific nodes mapped; real-Figma acceptance pending |
| Motion | Beta; styles, playhead, keyframes, timelines and animated export mapped/tested against fake runtime |
| Prototyping | Reactions, flows, transitions, overlays and validation mapped |
| Slides | Grid, rows, slide creation, view state and transitions mapped where public API permits |
| Buzz | Frame/instance creation, asset typing, text/media fields and smart resize mapped |
| Dev Mode / Codegen | CSS, dev resources, focused node, codegen state/refresh and gated manifests mapped |
| Text review / collaboration | Privileged surfaces capability-gated with dedicated manifests |
| Shaders | Discovery/import and fill/stroke/effect application mapped |
| Team library | Components, component sets, styles, variables and collections mapped |
| REST/cloud | 54 pinned official OpenAPI operations plus `cloud.status`; protected credential helper required |
| Linux Driver Host | Real sandbox conformance exercised in hosted CI |
| macOS/Windows Driver Host | Depends on current platform-host sandbox support; no real-Figma claim from compile evidence |
| Real Figma | Protected/manual acceptance still pending |

Plugin API coverage is pinned to `@figma/plugin-typings 1.139.0`; REST coverage is pinned to the recorded `figma/rest-api-spec` commit. Public upstream surfaces that cannot be created or mutated by the official API are classified rather than emulated through private APIs.

The bridge emits allowlisted selection/page/document-change events through Driver Protocol v2. Figma content remains untrusted data. Cooperative cancellation/progress remain negotiated only when their runtime contracts can be satisfied honestly.
