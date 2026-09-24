# Verification — Semwright 0.9.0-dev.1

**Accepted development baseline: the exact Git commit containing this document.**

**Verdict: all required hosted quality workflows are green on this exact commit; Semwright is
still a development snapshot and is not v1.0/release accepted.** Evidence from earlier commits is
historical only and is not used to certify this baseline.

## Exact-commit GitHub Actions evidence

| Workflow/job | Required result | Evidence location |
|---|---:|---|
| Quality gates / source contracts | PASS | Commit checks: `Quality gates` |
| Quality gates / static Ruff/ShellCheck/actionlint | PASS | Commit checks: `Quality gates` |
| Quality gates / Rust 1.88 MSRV | PASS | Commit checks: `Quality gates` |
| Quality gates / Rust x86_64 | PASS | Commit checks: `Quality gates` |
| Quality gates / Rust ARM64 | PASS | Commit checks: `Quality gates` |
| Dependency, coverage and fuzz / dependencies | PASS | Commit checks: `Dependency, coverage and fuzz gates` |
| Dependency, coverage and fuzz / coverage | PASS | Commit checks: `Dependency, coverage and fuzz gates` |
| Dependency, coverage and fuzz / bounded fuzz | PASS | Commit checks: `Dependency, coverage and fuzz gates` |
| Native application integration / Chromium | PASS | Commit checks: `Native application integration` |
| Native application integration / Driver conformance | PASS | Commit checks: `Native application integration` |
| Native application integration / Driver distribution | PASS | Commit checks: `Native application integration` |
| Native application integration / LibreOffice driver | PASS | Commit checks: `Native application integration` |
| Native application integration / Blender driver | PASS | Commit checks: Native application integration |
| Native application integration / KiCad + MLT drivers | PASS | Commit checks: Native application integration |
| Native application integration / X11 backend | PASS | Commit checks: Native application integration |
| Native application integration / AT-SPI GTK | PASS | Commit checks: Native application integration |
| Native application integration / AT-SPI Qt | PASS | Commit checks: Native application integration |
| Native application integration / PipeWire ScreenCast | PASS | Commit checks: Native application integration |
| Platformization / Linux regression | PASS | Commit checks: Platformization and macOS |
| Platformization / native macOS ARM64 | PASS | Commit checks: Platformization and macOS |
| Platformization / native macOS Intel | PASS | Commit checks: Platformization and macOS |
| Packaging certification / x86_64 | PASS | Commit checks: `Packaging certification` |
| Packaging certification / ARM64 | PASS | Commit checks: `Packaging certification` |

The development matrix uses Rust 1.98.1, while Rust **1.88.0 is the declared and executed MSRV**. The hosted MSRV job runs the required fmt/check/build/Clippy/tests/doctests/docs/release/fake/federation gates at that lower bound. The normal x86_64/ARM64 matrix runs the locked workspace on 1.98.1. Source contracts run Python discovery, Node tests, source/schema validation and the native C/openat2 harness; a separate hosted static-lints job executes pinned Ruff 0.13.2, ShellCheck and actionlint including embedded workflow shell.

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
hardening matrix additionally exercises per-file/count/total download quotas with CDP cancellation,
dead-instance/crash recovery and relaunch, screenshot/download artifact cleanup, stale references
and real multi-frame navigation. The runner normalizes the overly permissive mode of its ephemeral
Chrome installation; production validation continues to reject executables writable by group or
others.

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
MCP transports and input-required rounds remain follow-on work; broker jobs are mapped to MCP Tasks
as described in the Events and Jobs section below.

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

## Adversarial sandbox and plugin-attestation closure included in this development line

Plugin Protocol v2 now binds the owner-reviewed manifest to the child binary's plugin name, plugin
version and SHA-256 digest of the complete ordered command descriptors before any plugin command can
execute. Hosted mismatch tests prove version or descriptor drift fails closed rather than accepting
an older v1-style identity-only handshake.

The hosted `driver-conformance` job also executes deliberately hostile plugin and DriverProvider
fixtures through the production Linux Bubblewrap + Landlock launcher. The fixtures prove granted
read/write mounts behave as declared while writes outside grants, host-secret reads, host PID
visibility and host-loopback connections are denied. Environment inheritance is reduced to the
sandbox-controlled allowlist; the DriverProvider fixture additionally observes its requested
RLIMIT_NOFILE bound. Timeout/provider shutdown tests spawn descendants and verify they cannot survive
long enough to mutate a writable grant. These are executed regression checks for the configured
sandbox boundary, not a formal proof against kernel, Bubblewrap, Landlock or native-code defects.

## Driver distribution closure included in this development line

The App Driver SDK now has owner-facing static/local distribution that is deliberately separate
from broker authority. `.swdp` v1 is not an arbitrary archive: it contains one bounded metadata
document and exactly one ELF payload, eliminating package-controlled extraction paths, symlinks and
install hooks. The package and executable are SHA-256 pinned; the index independently pins package
size/hash, driver identity/version/publisher, a Semwright SemVer requirement and optional exact
application versions.

The hosted `driver-distribution` job runs the registry tests plus a real CLI smoke path that packages
`/usr/bin/true`, builds/validates a local index, resolves compatibility, dry-runs installation,
installs into a private temporary XDG store, verifies the installed bytes and `0700`/`0600` modes,
then removes the receipt-bound version. Tests reject traversal, symlink escape, tampering, malformed
package lengths, duplicate entries, invalid digest forms, incompatible/missing application versions
and version-path escape during removal. A separate update test installs 1.0.0 then 2.0.0 and verifies
the stable manifest moves to 2.0.0 while the older version directory remains.

Distribution establishes integrity and compatibility, not publisher identity or execution authority:
install/update do not execute the payload, run conformance, edit daemon policy or create a
`driver:<id>` grant. Runtime `DriverProvider` digest validation and Bubblewrap + Landlock remain the
execution boundary. Remote index transport, a hosted marketplace and cryptographic publisher
signatures are not certified by this local/static v1.

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

The MCP adapter now maps Semwright jobs to the negotiated `io.modelcontextprotocol/tasks`
extension using the official Rust SDK: task creation is opt-in, Task IDs are the session-scoped
JobStore IDs, `tasks/get` and `tasks/cancel` re-enter normal broker policy, legacy clients are
rejected for task creation, and the official-SDK E2E exercises create/poll/result/cancel behavior.
Semwright does not fabricate `input_required` transitions that its broker cannot currently emit.

This does not certify a universal provider progress percentage/artifact contract, remote durable task
persistence or negotiated driver-child job/event/cancellation interfaces. Those remain follow-on
compatibility work rather than implied capabilities of the core job store.

## OBS deep-driver closure included in this development line

The workspace now includes a curated OBS Studio driver over obs-websocket 5.x with **65**
strict Semwright capabilities covering status, scenes, scene items, inputs/audio, filters,
transitions, recording, streaming, replay buffer, virtual camera, media and Studio Mode.
The driver maintains bounded connection generations, request correlation, local refs,
preconditions, reconnect state, event backpressure and output lifecycle state without exposing
an arbitrary raw obs-websocket request gateway.

The dedicated `OBS driver integration` workflow executes the production Rust client against an
independent Python WebSocket fixture, including authentication, out-of-order/late/duplicate
responses, reconnect generations, bounded event floods, malformed wire data, concurrency and
shutdown. It also runs the driver through the real Semwright Driver Host, Bubblewrap + Landlock,
broker policy and CLI path, and executes six bounded OBS fuzz targets.

The feature-introduction real-obs job additionally starts a disposable OBS Studio 30.0.2 instance with
obs-websocket 5.3.4 inside a private user/network namespace and Bubblewrap filesystem view. It uses
a temporary HOME/XDG tree, an isolated Xvfb display, loopback networking only, no camera/microphone,
no user profile and no external streaming target. The production Rust probe authenticates and
successfully executes read-only `GetVersion` and `GetSceneList`; the accepted evidence explicitly
records `recording_started=false` and `streaming_started=false`.

This does **not** imply that Driver Protocol v1 transports driver-child events, cooperative
cancellation, dynamic capability changes or provider-wide progress/artifacts. Those generic
protocol gaps remain fail-closed/follow-on work rather than being simulated by the OBS driver.

## Blender, KiCad and MLT deep-driver closure included in this development line

The sandboxed Blender DriverProvider executes against real Blender 4.5.14 with bounded curated
operations plus RNA/operator/add-on introspection. Hosted integration mutates objects/materials,
renders a deterministic small image and saves a real .blend. A separate Xvfb-backed interactive
add-on smoke exercises the legacy in-process bridge through the broker and Blender main-thread
timer. Neither path exposes arbitrary Python or generic operator execution by default.

KiCad and MLT provide two additional integration shapes over the same Driver SDK. KiCad exercises
structured project/document semantics and compatibility fixtures; the MLT driver models timelines,
tracks, clips, transitions/effects, frame-rational timing, Kdenlive/Shotcut compatibility and
round-trip preservation. The certified MLT line executes against a real melt runtime, not only XML
fixtures.

## Universal Linux runtime closure included in this development line

The X11 fallback no longer performs unbounded synchronous x11rb work on the async executor.
Operations cross a bounded blocking boundary with timeout/cancellation semantics, and window refs
carry lifecycle epochs. Hosted Xvfb integration exercises discovery, create/destroy/reuse and stale
identity behavior.

AT-SPI now supports revisioned semantic snapshots, deltas, structural resync and targeted
stale-reference invalidation. Dedicated hosted jobs execute real disposable GTK and native Qt
fixtures through an accessibility bus, mutate editable text, observe deltas, terminate the
application and prove old refs become stale.

A separate **real GNOME Wayland** execution on Ubuntu 24.04.1 / GNOME Shell 46.0 runs the same
production AT-SPI path against Zenity 4.0.1 in the active `wayland-0` login session on commit
`6bab0cc`. Semwright discovers the application, takes a complete semantic snapshot, mutates the
editable text through AT-SPI (without global keyboard/pointer injection), observes a delta, closes
the fixture, forces structural resync and rejects the old ref as stale. Sanitized evidence is stored
in `verification/live-gnome/gnome-wayland-atspi.json`. This certifies the GNOME semantic GTK route,
not the optional GJS bridge, portal input consent or the remaining Plasma/Sway/Hyprland matrix.

The RemoteDesktop EIS sender is implemented in the platformized Linux host and a real EIS protocol
fixture negotiates a sender session and transmits keysym, UTF-8 text, relative pointer motion,
buttons and scrolling. A real user-approved desktop-portal ConnectToEIS session is still pending
and remains a release blocker.

## PipeWire ScreenCast closure included in this development line

The Linux platform host implements owner-scoped XDG ScreenCast sessions plus bounded PipeWire raw
frame capture. The exact-commit native job publishes a real synthetic PipeWire source, negotiates
the stream, copies a frame through the production capture code, validates supported packed formats,
stride/bounds behavior and writes a private PNG artifact. Stream cancellation, timeout and cleanup
are bounded. This closes the missing pixel-decoder implementation gap but does not claim that every
desktop/compositor portal path has been exercised live.

## Portal persistence and clipboard closure included in this development line

RemoteDesktop restore-token state supports private process and durable modes, atomic owner-only
storage, token rotation/single-use semantics and explicit clearing without exposing token contents.
Clipboard read/write is integrated into the consented RemoteDesktop session rather than a separate
implicit authority path. Private D-Bus portal fixtures execute restore rotation and clipboard grant
lifecycle, including cleanup. Real user-facing portal consent/revocation across the desktop matrix
remains part of the live-session release gate.

## Platform host and macOS foundation included in this development line

The portable-core/platform-host split now executes Linux regression jobs and native macOS jobs on
both Apple Silicon and Intel hosted runners. The macOS foundation includes native host plumbing and
cross-architecture compilation without weakening Linux-only driver behavior. This is a platform
foundation, not a claim that macOS has feature parity with the Linux semantic host.

## Reproducible packaging and user-install certification included in this development line

The hosted `Packaging certification` workflow runs on native x86_64 and ARM64 Linux runners. It builds the five release executables (`semwright`, `semwrightd`, `semwright-mcp`, `semwright-inspect`, and `semwright-sandbox`), creates normalized tar/deb artifacts twice, compares their hashes, validates package payloads, and exercises a private user install -> execute -> uninstall lifecycle. The uninstall regression also proves modified/tampered installed files are refused rather than deleted blindly.

This closes Semwright's `release_packaging_validation` gate and the development evidence gap for native tar/deb packaging, reproducibility and user install/uninstall. It does **not** claim Nix evaluation, publisher identity, SBOM generation, signing/notarization or publication provenance; those remain separate release/security work and `release-readiness.json` remains fail-closed.

## Verification hardening included in the baseline

- The command schema contract expects the current 90 descriptors (180 input/output schemas).
- The local runner bounds time and output, records real exit codes and hashes, persists transitions,
  rejects contradictory PASS reports, and does not overwrite prior evidence.
- Release admission has an independent required-gate set and rejects malformed/partial metadata,
  non-boolean gates, floating toolchains, symlinks, and invalid lockfiles.
- The verifier is a trusted-tool process controller, not a sandbox for hostile plugins.
- The development checkout remains intentionally blocked by `release-readiness.json`; green CI is
  necessary but does not itself authorize a release.

## Evidence boundaries

This baseline does **not** claim Plasma Wayland, Sway, Hyprland or a complete native-desktop
X11 matrix, nor a real user-approved portal ConnectToEIS session. GNOME Wayland has a real semantic
GTK/AT-SPI execution, but that does not certify every GNOME extension/portal/scaling path. It does
not certify a sandbox for same-UID MCP upstream executables, a remote signed driver marketplace or
cryptographic publisher identity. Adversarial plugin/driver sandbox regressions are executed but do
not constitute a formal security proof. Rust 1.88 is the executed MSRV, Chromium quota/crash/frame
hardening, MCP Tasks mapping, reproducible native tar/deb packaging and private user install/uninstall
are executed. Nix evaluation, SBOM/signing/publication provenance, independent security review,
provider-wide progress/artifacts and negotiated dynamic driver child event/cancellation interfaces
remain follow-on work.

Local exploratory evidence and `dummy-docs/` are intentionally excluded from Git. Historical
failed logs remain useful diagnostics but do not contribute to the accepted baseline. See
[ACCEPTANCE.md](ACCEPTANCE.md) and [RELEASE_BLOCKERS.md](RELEASE_BLOCKERS.md) for the remaining
scope.
