# Research baseline — 2026-09-28, role F

## Repository evidence
Baseline b736d41b61c4a4146c9e75c16796e251b025e69f; A C0 26602e4b25929be869d69ef28fef4dd9713180d7; observed A tip 7ed5b848e4d2e7af235d6166e6f93e0cf0bac90d; C P0 6ee52b428310370d3ad438a13964086a63f48367. C0/C1 handoff documents, actual model/canonical/vault/controller code and consumer tests were read. No extra kernel or report enum is needed.
Godot #171 is already in main. scene_save_ops.gd saves only the scene with flags=0 and compares external-resource hashes. readback_ops.gd reads typed native properties. Existing tests use same-process reload; F adds a fresh-process harness. Godot's animation pagination documentation explicitly warns that matching scene stamps are not transactional snapshots of in-place external animation edits.
Blender #154 remained OPEN at 74671c11dda2133ce6af939896c49cdbb6ba47d5 during inspection. The fixed Python GLB Commands adapter already exists in the frozen main snapshot; F does not reapply that feature. Native decoder membership and excluded-resource mutants are additional obligations, not inherited evidence from #154.

## Official sources and design decisions
https://docs.godotengine.org/en/4.7/classes/class_resourcesaver.html — consulted for ResourceSaver.save and flags. Bundling/external-resource semantics are distinct from scene-only save. Runtime CLI success and ResourceSaver acknowledgment are not independent persistence proof.
https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows — consulted for push/PR/dispatch and event SHA behavior. The new F workflow uses its own branch's push trigger rather than merging main to enable dispatch.
https://docs.github.com/en/actions/how-tos/manage-workflow-runs/re-run-workflows-and-jobs — reruns preserve original source identity; new fixes require a new source run.
https://cli.github.com/manual/gh_run_rerun — use database job IDs and inspect actual dependencies; do not rerun old jobs to claim new-SHA verification.
https://docs.blender.org/api/4.5/bpy.ops.export_scene.html — API reference remains the design source for export behavior. The official Blender 4.5 release index and `blender-4.5.14.sha256` were rechecked on 2026-09-28; the Linux x64 archive hash is `9ba871ff2ecd36526b77432745980b7e6664ecd0c7ca11c48849073dcfe06da3`.

## Runtime and provenance limits
Godot 4.7.2-stable and Blender 4.5.14 are pinned from official release sources. GitHub release metadata for `Godot_v4.7.2-stable_linux.x86_64.zip` reports SHA-256 `cadd3204e728a35d3f13adb7fd0d7902636b79f6b95c40c265eb73b6c35329e4`; F verifies that digest against the downloaded archive before extraction, then records (but does not pre-trust) the extracted binary digest. Blender Linux x64 remains `9ba871ff2ecd36526b77432745980b7e6664ecd0c7ca11c48849073dcfe06da3`. These are configuration pins, not evidence that F's native lane passed. Rust 1.98.1 is the repository toolchain; E0 CI reported rustc 1.98.1 (48a229cea 2026-09-01).
No dependency upgrades or third-party code copies were introduced by F. New code inherits the workspace license. Current-SHA dependency/license audit and native acceptance remain outstanding; documentation is not a substitute for them.
