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
| Native application integration / Driver conformance | PASS | Commit checks: `Native application integration` |
| Native application integration / LibreOffice driver | PASS | Commit checks: `Native application integration` |

The quality matrix uses Rust 1.98.1 and runs, with the locked dependency graph, `fmt`,
`check`, debug build, Clippy with warnings denied, workspace/all-target tests, doctests,
rustdoc with warnings denied, release build, and the fake daemon/CLI/recipe/audit smoke path on
x86_64 and ARM64. Source contracts run Python discovery, Node tests, source/schema validation,
and the native C/openat2 harness.

The dependency job runs `cargo audit --deny warnings` and `cargo deny --locked check`. Coverage
produces workspace LCOV and JSON artifacts; no percentage is asserted here. The fuzz job executes
the `protocol`, `selector`, `recipe`, `plugin`, and `path` targets for bounded intervals. Workflows
use explicit Bash, so a producer failure cannot be hidden by `tee`.

The native Chromium job launches the Rust adapter against the hosted runner's real Chrome binary
using a disposable owned profile and loopback fixture. It exercises launch, operation-specific
availability, tab navigation, native input, DOM snapshot, screenshot artifact metadata, stale refs,
origin denial, download denial, and profile cleanup. A Chrome 152 target-metadata race discovered
during this pass is covered by a bounded stabilization regression: transient unparsable target
metadata may be retried, while malformed user URLs and disallowed origins remain fail-closed. The
runner normalizes the overly permissive mode of its ephemeral Chrome installation; production
validation continues to reject executables writable by group or others.

## Provider Runtime closure included in this development line

The Provider Runtime is now an explicit broker abstraction rather than command-prefix inference.
Provider identity, source kind, version, namespace and origin are owner-bound; imported metadata
is untrusted data and cannot claim builtin authority. Provider capabilities can be registered,
replaced and removed atomically against a catalog revision, and invocation provenance is preserved
through execution and audit.

The exact-commit quality suite exercises **12 Provider Runtime integration tests** and **10 dynamic
provider catalog tests**. These include simultaneous registration, operation-level availability,
atomic descriptor replacement, stale catalog revisions, schema/result validation, timeout and
cancellation propagation, hostile metadata, provider-scoped events, definitive disconnect, and
bounded external JSON Schema/value traversal including Draft 7 `dependencies`. The real Chromium
hosted job also exercises the acknowledged-close stale-reference regression.

Provider Runtime is the common authority boundary used by the federation and driver layers below.

## MCP federation closure included in this development line

Governed local stdio MCP federation is implemented as an `ExternalMcpProvider`, not as a bypass
around the broker. Owner-pinned upstream definitions negotiate through the official MCP SDK,
import bounded/namespaced tools as untrusted capabilities, and execute through the normal
policy/approval/audit path. Integration tests exercise policy denial, cancellation, malformed
descriptors/results, `tools/list_changed` refresh, crash invalidation and owner-registry lifecycle.

This certifies the mediated federation path, not the upstream executable itself. A trusted stdio
upstream still runs as the same Unix user and is not currently sandboxed against that UID. Remote
MCP transports, task/job bridging and input-required rounds remain follow-on work.

## App Driver SDK closure included in this development line

The persistent App Driver SDK/host is implemented on the same Provider Runtime. A strict manifest
binds owner-assigned identity, protocol/version, application metadata, requested resources and the
SHA-256 of an owned/root ELF. The host stages the verified bytes and refuses unsandboxed execution;
the conformance fixture runs through bubblewrap plus Semwright's Landlock helper with a scrubbed
environment and isolated network by default.

The hosted `driver-conformance` job executes a real persistent fixture through handshake,
capability digest attestation, health, a safe read-only operation and clean shutdown. It also
executes the broker smoke path and compiles a newly scaffolded driver. Protocol v1 deliberately
rejects dynamic capabilities, provider events and cooperative cancellation until those interfaces
are negotiated and tested.

## LibreOffice deep-driver closure included in this development line

LibreOffice is the first accepted deep application driver built on the public App Driver SDK that
is neither the browser adapter nor the Blender prototype. The owner-pinned driver runs persistently
inside Semwright's Bubblewrap + Landlock path, launches a private headless LibreOffice/UNO process,
and receives only the workspace plus explicitly granted read-only `/etc/libreoffice` and `/etc/fonts`
configuration mounts. Driver-requested RLIMITs are bounded again by the sandbox helper.

The hosted `libreoffice-driver` job installs real Writer/Calc and `python3-uno`, preflights
unprivileged Bubblewrap/AppArmor behavior, and executes both the direct sandbox integration and the
full CLI -> daemon -> broker -> `DriverProvider` -> UNO path. The verified capability set covers
status, Writer create/read, Calc create/get/set, and PDF export. Evidence includes Writer roundtrip,
numeric zero preservation, Calc mutation, PDF structure and refusal to overwrite an existing target.
This certifies the listed operations against the hosted LibreOffice version; it is not a claim that
the entire UNO object model is exposed or that arbitrary macros/scripts are permitted.

## Events and jobs closure included in this development line

The broker now carries typed provider/source provenance on events while preserving the existing
sequence/replay wire. Replay and live delivery enforce optional session audience, so private job
lifecycle events are not visible to other broker sessions. Provider payload metadata remains
explicitly untrusted and cannot overwrite reserved provenance fields.

The built-in `jobs.start`, `jobs.get` and `jobs.cancel` commands implement bounded, session-scoped
long-operation state. Nested requests re-enter the normal broker execution path and therefore keep
schema validation, policy, confirmation, provider provenance and audit. Tests cover read-only
completion, mutation denial from an observe-only session, cross-session privacy, revocation,
idempotent cancellation and cancellation of a blocked dynamic provider without waiting behind its
execution gate. Retention is bounded and oversized completed result bodies are omitted rather than
stored indefinitely.

This does not certify a universal provider progress percentage, artifact model, remote task
persistence, automatic MCP Task mapping or negotiated driver job/event interfaces. Those remain
follow-on compatibility work rather than implied capabilities of the core job store.

## Verification hardening included in the baseline

- The command schema contract expects the current 86 descriptors (172 input/output schemas).
- The local runner bounds time and output, records real exit codes and hashes, persists transitions,
  rejects contradictory PASS reports, and does not overwrite prior evidence.
- Release admission has an independent required-gate set and rejects malformed/partial metadata,
  non-boolean gates, floating toolchains, symlinks, and invalid lockfiles.
- The verifier is a trusted-tool process controller, not a sandbox for hostile plugins.
- The development checkout remains intentionally blocked by `release-readiness.json`; green CI is
  necessary but does not itself authorize a release.

## Evidence boundaries

This baseline does **not** claim live GNOME, Plasma, Sway, Hyprland, native X11, portal EIS,
PipeWire pixel streaming, persistent portal restore tokens, real Blender, hostile plugin-sandbox
certification, a sandbox for same-UID MCP upstream executables, or a distributed driver registry.
It does not establish an MSRV, reproducible binary packaging, installation, SBOM/signing or an
independent security review. Provider-specific progress/artifacts, MCP task mapping and negotiated
dynamic driver job/event interfaces remain follow-on work.

Local exploratory evidence and `dummy-docs/` are intentionally excluded from Git. Historical
failed logs remain useful diagnostics but do not contribute to the accepted baseline. See
[ACCEPTANCE.md](ACCEPTANCE.md) and [RELEASE_BLOCKERS.md](RELEASE_BLOCKERS.md) for the remaining
scope.
