# Release impact

PR #175 is the E development PR. Source commits exist, but BLENDER_AUTHORING_READY is not declared and main must not consume this branch merely because code is present.

## Compatibility

Existing Blender capabilities retain their public names. The PR adds `driver.blender.composition.{inspect,plan,apply,measure,validate,verify,persist,reopen}`; authoring calls require protocol-v2 authenticated execution context. Legacy context-free execution explicitly refuses those new mutations.

PR #154's `driver.blender.export.glb` is reused. E adds a conservative dependency preflight before that exporter. Content previously accepted solely because it was selected can now be denied when linked/override data, external parents/targets, unmanaged modifiers/constraints, unrelated action expansion, unsupported shader graphs or out-of-workspace textures make effective membership unprovable. This is intentional fail-closed behavior and requires native regression evidence.

The driver now consumes `semantic-composition`, `media-time`, C's `project-graph` and F's `effect-conformance`. The workspace manifest adds only F's existing crate as a workspace dependency alias. Cargo.lock changes list existing workspace packages; their correctness is still subject to exact-SHA Actions with `--locked`.

## Support claim

The first native target is Linux with pinned Blender 4.5.14 LTS inside the existing Driver Host sandbox. A successful Rust compile is not Blender acceptance. A successful Blender workflow is not Godot roundtrip, packaging, hostile/fuzz or R16 security closure. Windows/macOS authoring support is not claimed by this PR.
