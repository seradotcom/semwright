# Godot semantic authoring runbook

## Workstation rule

Use the D worktree for reading/editing, Git/GitHub administration, hashes and lightweight syntax checks only. Do not run Cargo build/check/test/clippy/doc, Godot, fuzzing, coverage, mutation or package builds on the workstation. Heavy work runs on GitHub-hosted Actions.

## Iteration scope and exact-SHA certification

The owned workflow is .github/workflows/godot-authoring.yml. Normal pushes are **iteration runs**, not certification. A lightweight `scope` job compares the pushed SHA with `github.event.before`, writes `verification/godot-authoring/scope.json`, and enables only affected lanes among godot-model, godot-native-authoring, godot-persistence, godot-export, godot-hostile and godot-cross-app-glb. Cross-app selection implies native selection. The shared `authoring_host.rs` is classified by changed Rust function/hunk: native-only scenario edits do not drag persistence/export/cross-app, while changes to their dedicated tests select those lanes and shared-helper edits conservatively select every host-backed lane. Iteration pushes share a cancel-in-progress concurrency group, so a newer D push can replace obsolete diagnostics instead of filling the runner queue. Documentation-only or delivery-only edits may legitimately stop after the scope artifact.

Full D certification is reserved for an exact integration candidate. Before this workflow exists on the default branch, the final candidate commit includes the literal marker `[godot-certify]` in its commit message; that push forces all six gates, uses a certification concurrency group keyed by the immutable SHA, and cannot be cancelled by later iteration pushes. Once the workflow exists on the default branch, `workflow_dispatch` may instead be used with required `expected_sha`; the selector rejects the run unless `expected_sha == github.sha` of the selected ref.

`godot-source-package` is certification-only. It runs only when scope reports `certify=true` **and** all six gates, including the dependent cross-app oracle, conclude `success`. It emits `certification.json` with the exact SHA, Actions run/attempt and all gate results alongside the Skill/source package. Iteration artifacts remain useful diagnostics but are not certification evidence for integration.

Inspect runs with:
    gh run list --repo seradotcom/semwright --workflow "Godot semantic authoring diagnostics" --branch feat/godot-semantic-authoring --limit 5 --json databaseId,headSha,status,conclusion,url
    gh run view RUN_ID --repo seradotcom/semwright --json headSha,status,conclusion,jobs,url

For a failed iteration job, read only that affected job log, identify step/test/source SHA, fix the concrete cause, and let the next push select the necessary lanes. Do not run the full matrix merely to debug an isolated model/native/persistence/export/hostile change. For the integration candidate, do not accept prior green iteration jobs from other SHAs as substitutes: the exact certification run must carry all final gates and package evidence on one SHA.

## Native product acceptance

The native lane uses the repository Driver Host sandbox setup, pinned Godot 4.7.2 and pinned Linux export template. The harness provisions grants/runtime/input fixtures only; projects/scenes/scripts are created by Broker -> Driver Host -> D capabilities.

Acceptance sequence: discover catalog; plan typed intent and prove plan makes no target bytes; apply; inspect/validate; native inspect; use `composition.native.query` to fetch one logical node and one persistent resource with selected properties without returning the full projection; page a >64-track animation through `composition.native.tracks.page` and a >64-key track through `composition.native.keys.page` across fresh inspect processes, reaching both final items; save/reopen in two processes; bounded playtest; managed project validate/run; export; launch package without editor/Semwright. For D12, after Driver Host has already passed on the native-authoring runner, pin and verify E's artifact, move the GLB through generic artifact.handoff, perform the incremental replacement, native persistence/play and `composition.verify` C receipt check on that same runner. The dependent cross-app oracle must then download the exact-SHA native artifact, redownload E, compare bytes, and validate both D12 and C receipts before delivery can run.

## Delivery package

The gated delivery job first builds the Semwright CLI, validates and inspects `semwright-godot-production`, emits a standalone Skill bundle plus SHA-256, and then runs `scripts/godot-authoring/package_source.py`. The source packer is GitHub-hosted-only: it reconstructs D from immutable Git objects, verifies A/C/F tree pins, checks patch application byte-for-byte and emits deterministic source ZIP plus manifest/SHA-256. It excludes .git, target, runtimes, import caches, exports, credentials and user files. Packaging is not native acceptance by itself.
