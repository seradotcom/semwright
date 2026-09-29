# Godot semantic authoring runbook

## Workstation rule

Use the D worktree for reading/editing, Git/GitHub administration, hashes and lightweight syntax checks only. Do not run Cargo build/check/test/clippy/doc, Godot, fuzzing, coverage, mutation or package builds on the workstation. Heavy work runs on GitHub-hosted Actions.

## Exact-SHA diagnostics

The owned workflow is .github/workflows/godot-authoring.yml. A source push runs independent godot-model, godot-native-authoring, godot-persistence, godot-export, godot-hostile and godot-cross-app-glb jobs. godot-source-package depends on all six acceptance lanes; skipped or failed prerequisites do not satisfy delivery.

Inspect runs with:
    gh run list --repo seradotcom/semwright --workflow "Godot semantic authoring diagnostics" --branch feat/godot-semantic-authoring --limit 5 --json databaseId,headSha,status,conclusion,url
    gh run view RUN_ID --repo seradotcom/semwright --json headSha,status,conclusion,jobs,url

For a failed job, read only that job log, identify step/test/source SHA, fix the concrete cause, commit/push a new SHA and inspect the new run. Cancel only D runs superseded by a newer D SHA.

## Native product acceptance

The native lane uses the repository Driver Host sandbox setup, pinned Godot 4.7.2 and pinned Linux export template. The harness provisions grants/runtime/input fixtures only; projects/scenes/scripts are created by Broker -> Driver Host -> D capabilities.

Acceptance sequence: discover catalog; plan typed intent and prove plan makes no target bytes; apply; inspect/validate; native inspect; page a >64-track animation through `composition.native.tracks.page` and a >64-key track through `composition.native.keys.page` across fresh inspect processes, reaching both final items; save/reopen in two processes; bounded playtest; managed project validate/run; export; launch package without editor/Semwright. GLB enters through generic artifact.handoff into the D input grant, with SHA pinned before plan/apply/native inspect.

## Delivery package

The gated delivery job first builds the Semwright CLI, validates and inspects `semwright-godot-production`, emits a standalone Skill bundle plus SHA-256, and then runs `scripts/godot-authoring/package_source.py`. The source packer is GitHub-hosted-only: it reconstructs D from immutable Git objects, verifies A/C/F tree pins, checks patch application byte-for-byte and emits deterministic source ZIP plus manifest/SHA-256. It excludes .git, target, runtimes, import caches, exports, credentials and user files. Packaging is not native acceptance by itself.
