# Verification — Semwright 0.9.0-dev.1

**Accepted development baseline: the exact Git commit containing this document.**

**Verdict: all required hosted quality workflows are green on this exact commit; Semwright is
still a development snapshot and is not v1.0/release accepted.** Evidence from earlier commits is
historical only and is not used to certify this baseline.

## Exact-commit GitHub Actions evidence

| Workflow/job | Required result | Evidence location |
|---|---:|---|
| Quality gates / source contracts | PASS | Commit checks: `Quality gates` |
| Quality gates / Rust x86_64 | PASS | Commit checks: `Quality gates` |
| Quality gates / Rust ARM64 | PASS | Commit checks: `Quality gates` |
| Dependency, coverage and fuzz / dependencies | PASS | Commit checks: `Dependency, coverage and fuzz gates` |
| Dependency, coverage and fuzz / coverage | PASS | Commit checks: `Dependency, coverage and fuzz gates` |
| Dependency, coverage and fuzz / bounded fuzz | PASS | Commit checks: `Dependency, coverage and fuzz gates` |
| Native application integration / Chromium | PASS | Commit checks: `Native application integration` |

The quality matrix uses Rust 1.98.1 and runs, with the locked dependency graph, `fmt`,
`check`, debug build, Clippy with warnings denied, workspace/all-target tests, doctests,
rustdoc with warnings denied, release build, and the fake daemon/CLI/recipe/audit smoke path on
x86_64 and ARM64. Source contracts run Python discovery, Node tests, source/schema validation,
and the native C/openat2 harness.

The dependency job runs `cargo audit --deny warnings` and `cargo deny --locked check`. Coverage
produces workspace LCOV and JSON artifacts; no percentage is asserted here. The fuzz job executes
the `protocol`, `selector`, `recipe`, `plugin`, and `path` targets for bounded intervals. Workflows
use explicit Bash, so a producer failure cannot be hidden by `tee`.

The native job launches the Rust Chromium adapter against the hosted runner's real Chrome binary
using a disposable owned profile and loopback fixture. It exercises launch, operation-specific
availability, tab navigation, native input, DOM snapshot, screenshot artifact metadata, stale refs,
origin denial, download denial, and profile cleanup. The runner normalizes the overly permissive
mode of its ephemeral Chrome installation; production validation continues to reject executables
writable by group or others.

## Verification hardening included in the baseline

- The command schema contract expects the current 83 descriptors (166 input/output schemas).
- The local runner bounds time and output, records real exit codes and hashes, persists transitions,
  rejects contradictory PASS reports, and does not overwrite prior evidence.
- Release admission has an independent required-gate set and rejects malformed/partial metadata,
  non-boolean gates, floating toolchains, symlinks, and invalid lockfiles.
- The verifier is a trusted-tool process controller, not a sandbox for hostile plugins.
- The development checkout remains intentionally blocked by `release-readiness.json`; green CI is
  necessary but does not itself authorize a release.

## Evidence boundaries

This baseline does **not** claim live GNOME, Plasma, Sway, Hyprland, native X11, portal EIS,
PipeWire pixel streaming, persistent portal restore tokens, real Blender, or executed plugin
sandbox conformance. It does not establish an MSRV, reproducible binary packaging, installation,
SBOM/signing, benchmark results, or an independent security review. The MCP tests exercise the
Semwright server with an official client; they are not MCP federation tests.

Local exploratory evidence and `dummy-docs/` are intentionally excluded from Git. Historical
failed logs remain useful diagnostics but do not contribute to the accepted baseline. See
[ACCEPTANCE.md](ACCEPTANCE.md) and [RELEASE_BLOCKERS.md](RELEASE_BLOCKERS.md) for the remaining
scope.
