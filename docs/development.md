# Development and verification workflow

Start from the real repository state and use a separate branch/worktree. Record the exact
base SHA, changed paths and scope. Do not assume a historical PR or branch still represents
current code. Read [architecture](architecture.md), [contribution rules](../CONTRIBUTING.md)
and [verification](../VERIFY.md).

## Cheap checks first

The source checkout is much smaller than Rust targets and native applications. Tiny static
checks may run in your worktree:

```sh
git diff --check
python3 scripts/review/validate_r16.py
python3 -m unittest discover -s tests/python -p test_r16_closeout.py -v
```

The documentary validator checks the named public documents' local link targets and the
review-record shape; it does not crawl external links, prove every Markdown anchor or
certify security. Its self-tests reject duplicate/missing evidence and unsupported claims.
`Cargo.lock` is committed. `scripts/dev/bootstrap.sh` initializes dependencies only when
that lock is absent; it is not an ordinary build prerequisite.

## Select remote CI by impact

Use `scripts/dev/ci-affected-areas.py` with the actual diff paths, then inspect the selected
workflow's triggers/conditions. Ordinary PR iteration must not invoke every application
engine for a documentation change. CircleCI is suitable for configured compatible iteration
when an authorized connection and credits exist. GitHub Actions supplies final/platform
lanes. Never forge provider environment flags to bypass a CI-only guard.

Heavy cargo builds/tests, fuzzing, mutation testing, browser/Blender/Godot/media engines and
platform certification belong on authorized disposable runners. Standard public hosted jobs
are the intended route; do not select paid larger runners, change billing, install tools on
an owner's device or allocate new paid services as an incidental verification step.

The R16 documentary workflow uses a standard Ubuntu runner and no Actions cache/artifact
upload. Its optional positive smoke checks out frozen source separately from the review harness,
builds only the daemon/CLI, runs selected pure contract libraries and executes the existing
fake recipe. It does not run hostile payloads or replace the full release matrix.

## Component-specific lanes

Use the component workflow rather than inferring support from the generic matrix:

- Native SDK changes use `.github/workflows/native-sdk.yml`; changes that affect the real execution
  route also use `.github/workflows/native-sdk-host.yml`. The manual full-portability input expands
  the portable SDK lane to Ubuntu x64/ARM64, Windows x64/ARM64 and macOS arm64/x64.
- Windows platform changes use `.github/workflows/windows-platform.yml`. Interactive unlocked-desktop
  certification is deliberately separate in `.github/workflows/windows-interactive.yml`.
- Application-driver changes route through `.github/workflows/native-integrations.yml` and any
  application-specific workflow selected by `scripts/dev/ci-affected-areas.py`.
- Portable release-layout changes use `.github/workflows/v1-distribution.yml` and the packaging /
  supply-chain workflows selected for that diff.

A portable compile is not a live Host or interactive-desktop certificate. Keep the exact source SHA,
runner/profile and skipped-job disposition with every support claim.

## Source identity and conclusions

Record full source SHA, harness/suite SHA when different, job/run IDs, attempt, configuration,
real exit codes/counts and any artifact digest. A rerun tests the original source, not a later
fix. A workflow can be green with every heavy job skipped; inspect job/step dispositions.
Historical certificates remain useful antecedents but are not new executions on a later SHA.

Freeze functional code/configuration/documentation first. Add reports/manifests in a later
evidence-only commit or publish them separately to avoid self-referential hashes. Revalidate
source-affecting edits and request separate review of your own security-relevant changes.
An evidence validator is not a release-admission authority.

## Final admission

Read `release-readiness.json` and [release blockers](../RELEASE_BLOCKERS.md). The release
workflow has its own fail-closed guard; do not toggle gates to match a desired conclusion.
R16 review, technical acceptance, productivity evaluation, main merge and publication are
separate decisions. Physical/interactive platform acceptance cannot be replaced by hosted
headless tests. Preserve historical failures when a subsequent correction succeeds.
