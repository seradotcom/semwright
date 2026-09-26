# Motion Canvas semantic completeness

Status target: **semantic-complete for managed declarative Motion Canvas authoring on the pinned 3.17.2 surface**. This is intentionally not a claim that Semwright can safely round-trip arbitrary hand-written TypeScript, execute callbacks, or mirror every runtime/editor method.

## Audited upstream surface

The coverage contracts are generated/classified against the exact `@motion-canvas/2d` and `@motion-canvas/core` 3.17.2 typings and are checked in CI against the installed lockfile version.

`API_COVERAGE.json` classifies all 25 public 2D component exports. The current classification is 20 concrete managed node kinds, 3 abstract semantic substrates (`Shape`, `Curve`, `Bezier`), one compiler-managed scene root (`View2D`) and one excluded component (`Icon`). Across their own public Props fields: 91 are directly managed, 62 are represented by an equivalent canonical semantic value, 8 are explicitly mixed, 7 are compiler-owned and 7 are excluded by design. The mixed entries are decomposed again: 7 value variants are managed, 5 represented and 8 excluded, so a union type cannot hide an unsupported arm behind a generic “managed” label.

`CORE_API_COVERAGE.json` classifies all 15 root core exports, all 8 `ProjectSettings` fields, and every public export discovered under flow (13), transitions (6) and tweening (60). It distinguishes persistent authoring semantics from compiler/runtime machinery.

`AUX_API_COVERAGE.json` closes the remaining public `@motion-canvas/2d` root modules: code (47 reachable exports), curves (17), decorators (28), partials (31), scenes (3), utils (19) and `jsx-runtime` (4), for 149 additional version-pinned classifications. These exports are mostly runtime utilities or compiler-owned construction machinery; declarative pieces such as CodeRange selection, filters, Gradients and layout value types are linked back to their managed semantic representations.

## What is semantically editable

The managed node graph covers Node/Layout/Shape/Curve inheritance plus group, layout, rect, circle, line, text, code, SVG, image, video, LaTeX, camera, grid, polygon, path, cubic and quadratic Beziers, spline/knot and ray. A single version-pinned registry is authoritative for canonical property name, upstream name, typed value category, storage representation, enum domain and safe animatability.

Agents can discover this surface through `semantic.types`/`semantic.describe`, inspect a revision-bound node, read one canonical property and atomically set/reset it with exact fingerprint/ref preconditions. Semantic property writes go through the normal transaction engine, validation, diff, deterministic compiler and atomic store.

Project-level authoring covers size/FPS/background/color space, theme tokens, bounded project variables, scenes/order/duration, typed transitions, nodes/hierarchy, assets, one Motion Canvas project-audio track with bounded offset/unity gain, cues, grouped animations, standard fixed easing functions, reusable components and deterministic render plans. The node value codec preserves semantic unions rather than flattening them: numeric/percentage lengths, min/max content limits, flex basis keywords, reverse flex directions, baseline/space-evenly alignment, tri-state layout inheritance, preformatted text wrapping, corner spacing, filters, bounded arbitrary CodeRanges, segmented LaTeX and declarative color Gradients are all represented without arbitrary JavaScript.

## Deliberate exclusions

These are not treated as accidental completeness gaps:

- **Icon**: upstream constructs Iconify HTTP URLs; production rendering is `network=false`. Import a local SVG/image asset instead.
- **Pattern / CanvasImageSource styles**: colors and declarative Gradients are managed, but `Pattern` carries live DOM/canvas object identity. Use an attested local image node instead of serializing ambient browser objects.
- **dynamic CodeScope/CodeTag and custom CodeHighlighter objects**: managed code persists bounded text, ranges and a fixed language/highlighter allowlist; SignalValue-backed/custom executable objects remain outside the data model.
- **active/general SVG and unrestricted TeX**: the corresponding properties are classified `mixed`; Semwright manages structural non-active SVG plus a bounded MathJax command vocabulary (including segmented `string[]` LaTeX) and rejects active/external SVG or unrestricted macro/package surfaces.
- **callbacks** (`Node.spawner`, `Code.drawHooks`, flow `run/every/loop*`): accepting them would reintroduce arbitrary application execution.
- **shader source**: `Node.shaders` can carry executable shader programs and needs a separate trusted shader domain.
- **ambient/runtime configuration** (`Latex.renderProps`, custom logger, DOM `tagName`, experimental feature switches): not persistent bounded motion semantics.
- **physical spring generators and parameterized easing factories**: their callback/dynamic-duration model does not fit the exact bounded duration/frame contract. Fixed standard easing functions are managed instead.
- **arbitrary external TypeScript/custom packages/plugins**: `project.detect` inspects them without execution and root mutation remains fail-closed. Exact-version projects can host isolated Semwright-managed scene islands under `.semwright/motion`; the generated bridge is the only integration surface and human TypeScript remains opaque/preserved. Semwright never claims closures/side effects are structured editable data.

## Completeness invariant

A surface is considered complete only when:

1. the pinned upstream declaration is present in the executable component, core or auxiliary coverage matrix;
2. it is either mapped to a typed semantic representation, compiler/runtime-owned with evidence, or excluded with an explicit reason;
3. managed property mappings resolve bidirectionally to the Rust registry;
4. representative semantic-complete generated TSX typechecks/builds against real 3.17.2; and
5. existing stale-ref, atomic-write, diff/dry-run, security, fuzz and Driver Host render gates remain green.

This makes completeness versioned and testable. A future Motion Canvas upgrade must update the matrices and pass the coverage checker before CI can accept the new version.
