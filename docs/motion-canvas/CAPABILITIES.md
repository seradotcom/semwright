# Motion Canvas capabilities

Driver identity: `driver:motion-canvas`. Protocol catalog count: **17**. The catalog is intentionally compact: type-specific creation and patching are expressed as bounded `project.apply` operations instead of dozens of redundant setters.

| Capability | Risk / idempotency | Purpose |
|---|---|---|
| `driver.motion-canvas.doctor` | read_only / read_only | Bounded driver/runtime health and capability count. |
| `project.inspect` | read_only / read_only | Return the authoritative model, source fingerprint, generated inventory and revision-bound refs. |
| `project.validate` | read_only / read_only | Validate model and deterministic compiler without writing. |
| `project.create` | mutating_reversible / non_idempotent | Create `semwright-motion.json`; supports truthful dry-run. |
| `project.diff` | read_only / read_only | Apply a prospective transaction in memory and return semantic/generated impact. |
| `project.apply` | mutating_reversible / non_idempotent | Atomically apply a bounded semantic transaction; supports dry-run. |
| `scene.list` | read_only / read_only | List scenes and stable refs. |
| `node.list` | read_only / read_only | List nodes, optionally scoped by scene ref. |
| `asset.import` | mutating_reversible / non_idempotent | Import a bounded local PNG/SVG/MP4/WebM/WAV/Ogg/MP3 from the explicit read-only media grant into the managed project; supports dry-run. |
| `asset.list` | read_only / read_only | List managed local assets and hashes. |
| `cue.list` | read_only / read_only | List named timing cues. |
| `animation.list` | read_only / read_only | List declarative animations. |
| `render.plan` | read_only / read_only | Validate exact bounded frame range/resolution/alpha plan. |
| `render.start` | mutating_reversible / non_idempotent | Start a driver-local render job after source fingerprint verification. |
| `render.status` | read_only / read_only | Return observed render phase only; no invented percentage. |
| `render.cancel` | mutating_reversible / idempotent | Cancel the owned render process tree. |
| `render.result` | read_only / read_only | Return validated artifact metadata for a terminal job. |

Every descriptor has strict schemars-derived input/output schemas, owner namespace/scope, bounded timeout, risk, idempotency and truthful dry-run metadata. Descriptor SHA-256 pinning is enforced by Driver Protocol v1.

## Transaction operations

`project.apply` and `project.diff` accept at most 128 operations. Current semantic operations are:

```text
settings_patch      theme_patch
scene_create        scene_patch        scene_duplicate
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

These operations cover layouts, text, shapes, diagrams/lines, Code nodes, SVG/media references, cues, animation flow/presets, camera and reusable safe components. All object refs resolve against the original transaction snapshot; a caller cannot manufacture a same-transaction ref to bypass stale-reference rules.

## External projects

Arbitrary hand-written Motion Canvas TypeScript is not treated as structured editable data. The managed driver does not promise safe mutation of closures, side effects, custom components or arbitrary packages. External-project mutation remains fail-closed rather than exposing arbitrary TypeScript execution.
