# CI iteration vs exact-candidate certification

Semwright separates fast development feedback from integration certification.

## Iteration

Pull requests run only the checks associated with the changed area. `scripts/dev/ci-affected-areas.py` is the single path classifier used by the global Quality, Native integration, and Security workflows.

- Source contracts and static lint remain cheap cross-repository PR checks.
- Workspace-wide MSRV/Rust matrices run only when shared/core Rust surfaces are touched.
- Native application jobs run only for the affected driver/platform consumers. Shared Driver Host/SDK/platform changes deliberately fan out to all consumers that depend on that boundary.
- Dependency checks run when dependency manifests/lockfiles change.
- Fuzz runs when fuzz surfaces change.
- Full workspace coverage, packaging certification, and supply-chain certification are not iteration checks.

Area-specific workflows such as Godot, Motion Canvas, Blender, Figma, OBS, macOS, X11, and Plasma retain their own path triggers. Their commands and native evidence are unchanged.

## Exact integration candidate

`Exact candidate certification` is manually dispatched on the branch/ref that currently points at the intended integration candidate.

Required inputs:

- `source_sha`: exact 40-character candidate SHA.
- `base_sha`: exact integration base SHA.

The workflow fails before certification if checked-out `HEAD` is not exactly `source_sha`, if either SHA is malformed/missing, or if `base_sha` is not an ancestor of `source_sha`.

Preferred dispatch helper:

```bash
scripts/dev/certify-candidate.sh <ref> <source_sha> <base_sha>
```

The helper verifies the local ref/SHA/ancestry before asking GitHub to dispatch; the workflow independently repeats the same checks on the runner.

The final candidate always runs the complete global Quality, Native application integration, Dependency/Coverage/Fuzz, Packaging, Supply-chain, and maintainer secret-precheck workflows through their reusable `workflow_call` entry points. Specialized platform/driver workflows are additionally called when the candidate diff affects their area.

Release artifact admission remains a later release-stage gate (`release.yml`): it is intentionally not conflated with ordinary integration certification because it asserts release readiness and package publication conditions rather than code-integration readiness.

Final artifacts include:

- `candidate-scope-<SOURCE_SHA>` with `SOURCE_SHA`, `BASE_SHA`, changed paths, and affected-area classification.
- `candidate-certification-<SOURCE_SHA>` with `CERTIFICATION.json` and its SHA-256.
- The unchanged native/coverage/package/SBOM artifacts emitted by the called certification workflows themselves.

Do not infer a final PASS from a previous SHA, a cancelled run, or a narrow iteration lane. Any code change after certification requires a new candidate SHA and a new exact-candidate certification run.
