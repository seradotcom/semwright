# Motion Canvas capabilities

Driver identity: `driver:motion-canvas`. Protocol catalog count: **23**. The catalog is intentionally compact: type-specific creation and patching are expressed as bounded `project.apply` operations instead of dozens of redundant setters.

| Capability | Risk / idempotency | Purpose |
|---|---|---|
| `driver.motion-canvas.doctor` | read_only / read_only | Bounded driver/runtime health and capability count. |
| `semantic.types` | read_only / read_only | List the version-pinned managed node types and typed safe-property surface. |
| `semantic.describe` | read_only / read_only | Describe one managed node type and its upstream 3.17.2 property mapping. |
| `project.detect` | read_only / read_only | Detect managed/external project metadata and exact Motion Canvas dependency evidence without executing project code. |
| `project.inspect` | read_only / read_only | Return the authoritative managed model, source fingerprint, generated inventory and revision-bound refs. |
| `project.validate` | read_only / read_only | Validate model and deterministic compiler without writing. |
| `project.create` | mutating_reversible / non_idempotent | Create `semwright-motion.json`; supports truthful dry-run. |
| `project.diff` | read_only / read_only | Apply a prospective transaction in memory and return semantic/generated impact. |
| `project.apply` | mutating_reversible / non_idempotent | Atomically apply a bounded semantic transaction; supports dry-run. |
| `scene.list` | read_only / read_only | List scenes and stable refs. |
| `node.list` | read_only / read_only | List nodes, optionally scoped by scene ref. |
| `node.inspect` | read_only / read_only | Inspect one revision-bound node plus its semantic type descriptor. |
| `node.property.get` | read_only / read_only | Read one canonical registry-approved property. |
| `node.property.set` | mutating_reversible / non_idempotent | Atomically set/reset one canonical property with fingerprint/ref preconditions and dry-run. |
| `asset.import` | mutating_reversible / non_idempotent | Import a bounded local PNG/SVG/MP4/WebM/WAV/Ogg/MP3 from the explicit read-only media grant into the managed project; supports dry-run. |
| `asset.list` | read_only / read_only | List managed local assets and hashes. |
| `cue.list` | read_only / read_only | List named timing cues. |
| `animation.list` | read_only / read_only | List declarative animations. |
| `render.plan` | read_only / read_only | Validate exact bounded frame range/resolution/alpha plan. |
| `render.start` | mutating_reversible / non_idempotent | Start a driver-local render job after source fingerprint verification. |
| `render.status` | read_only / read_only | Return observed render phase only; no invented percentage. |
| `render.cancel` | mutating_reversible / idempotent | Cancel the owned render process tree. |
| `render.result` | read_only / read_only | Return validated artifact metadata for a terminal job. |

Every descriptor has strict schemars-derived input/output schemas, owner namespace/scope, bounded timeout, risk, idempotency and truthful dry-run metadata. Descriptor SHA-256 pinning is enforced by the Driver SDK; this driver currently negotiates protocol v1.

## Transaction operations

`project.apply` and `project.diff` accept at most 128 operations. Current semantic operations are:

```text
settings_patch      theme_patch          variable_set
variable_remove      scene_create        scene_patch        scene_duplicate
scene_remove        scene_reorder
node_create         node_patch         node_remove
node_reparent       node_reorder
animation_add       animation_patch    animation_remove
animation_group     animation_preset
cue_upsert          cue_remove
audio_set           asset_remove
code_highlight      camera_focus
diagram_edge_create component_create
```

These operations cover project variables, layouts, text, shapes, curves, diagrams/lines, Code nodes, SVG/media references, cues, animation flow/presets, camera and reusable safe components. The generic node-property capabilities are backed by the same version-pinned registry used by validation and code generation; they are not arbitrary property dispatch. All object refs resolve against the original transaction snapshot; a caller cannot manufacture a same-transaction ref to bypass stale-reference rules.

## Semantic substrate

The managed 3.17.2 substrate currently exposes 20 concrete node kinds: group, layout, rect, circle, line, text, code, svg, image, video, latex, camera, grid, polygon, path, cubic_bezier, quad_bezier, spline, knot and ray. `properties.semantic` carries only registry-approved bounded values; inherited Node/Layout/Shape/Curve properties retain their upstream mapping in the descriptor. The value codec preserves canonical meaning for numeric/percentage lengths, flex-basis/content keywords, tri-state layout inheritance, reverse flex directions, baseline/space-evenly alignment, `textWrap="pre"`, corner-spacing radius, filters, arbitrary bounded CodeRanges, segmented LaTeX and declarative Gradients. Pattern/CanvasImageSource, active SVG, unrestricted TeX, shader source and dynamic CodeTag/SignalValue objects remain explicit trust-boundary exclusions. `AnimatedProperty::Semantic(name)` is accepted only when the registry marks that canonical value safely interpolatable.

`API_COVERAGE.json`, `CORE_API_COVERAGE.json` and `AUX_API_COVERAGE.json` are executable coverage contracts. `mixed` union properties must classify each managed/represented/excluded arm independently. See `SEMANTIC_COMPLETENESS.md` for the exact boundary and exclusions.

## External projects

Arbitrary hand-written Motion Canvas TypeScript is not treated as structured editable data. The managed driver does not promise safe mutation of closures, side effects, custom components or arbitrary packages. External-project mutation remains fail-closed rather than exposing arbitrary TypeScript execution.
