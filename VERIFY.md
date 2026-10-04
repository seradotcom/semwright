# Verification — Semwright 0.9.0-dev.1

> Historical receipts under [`verification/`](verification/README.md) preserve their original source-scoped terminology and identifiers; they are evidence records, not current governance or product copy.

## Current staging policy

The [2026-10-03 release policy](docs/release-policy.md) supersedes the older live-matrix
publication prerequisite. R16 is CLOSED; R06/R18 residuals remain OPEN/deferred post-v1, not PASS.
Staging admission does not authorize publication. Independent security review is still mandatory;
`release-readiness.json` currently says BLOCKED_PENDING_SECURITY_REVIEW.
The new distribution contract requires complete extracted-bundle install/smoke/uninstall checks
on six native architectures, internal checksums and a global exact-SHA manifest. Historical runs
below do not certify this newer packaging implementation; use the staging PR/run evidence.

## Historical review snapshot versus historical evidence

The R16 review snapshot is `6491c0d838fa066938a494524d69ed507aa0dbe8`. The integrated
engineering source is `cd518748f742025a251b78028613aa1b16919e73`; the actual diff is four documentation files and
the Windows UIA test fixture, not production-source changes. Native Windows run
[37096430846](https://github.com/seradotcom/semwright/actions/runs/37096430846) passed on the
review snapshot. The separate global disposition preserves its original failed run and corrected
fixture retest. Current `main` has newer Windows evidence: run
[37228279724](https://github.com/seradotcom/semwright/actions/runs/37228279724) passed native x64,
native ARM64 and both sealed-tool compatibility jobs at
`04cf0ef7062d134b71206d832375be9545553bb4`. None of these records supplies an independent
R16 verdict or turns skipped main-push jobs into executed gates.

Consult the [integration ledger](docs/semantic-creation/INTEGRATION.md) and
[R16 evidence directory](verification/r16-closeout/README.md). The bounded R16 smoke is not
a repeat of every native, supply-chain, physical-desktop or fuzz gate below.

## Historical preflight at 241000c268d1bf1dc29d4e91a913097ac0d020cb

Historical coordination snapshots are intentionally not part of the public repository. Verification
claims are tied to exact source SHAs, GitHub Actions run identities and the durable R16 review evidence
under verification/r16-closeout. A PASS on one commit never certifies a later commit, and skipped or
blocked checks are never treated as successful execution.

## Required GitHub Actions gates

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
| V1 distribution / six native platforms | PASS | Exact-main run `37146331051` at `9954c1f95f68305f32f153fe5ab302441845b7ed`: Linux x64/ARM64, Windows x64/ARM64 and macOS arm64/x64 success |
| V1 distribution / manifest | PASS | Run `37146331051`: manifest job success; retained distribution manifest artifact digest `sha256:1fe1f4492e8b4526edc9565bca395e54cc45f551b27b3179a56e65362926bc7f` |
| Windows / x64 native noninteractive | PASS | Current-main run `37228279724`, job `111512424502`: success |
| Windows / ARM64 native noninteractive | PASS | Current-main run `37228279724`, job `111512424275`: success |
| Windows / sealed-tool compatibility | PASS | Run `37228279724`, jobs `111512424435` and `111512424528`: success; not interactive certification |
| Native SDK / repository, file profile, Driver/Graph, TypeScript binding and clean-room package | PASS | Public integration run `37181039129` at `09f71d490ac86f8f8e86dcda6c2552f50c59d487`: all jobs success |
| Native SDK / real Host E2E | PASS | Public integration run `37181039113` at the same SHA: `native-host` success |
| Native SDK / six-platform portable baseline | PASS | Full-portability run `37179820287`: Ubuntu x64/ARM64, Windows x64/ARM64 and macOS arm64/x64 all success |
| Supply-chain / Nix, bundles and attestations | PASS | Observed run `36394993370`: success |
| Godot / conformance and real runtime | PASS | Observed run `36394993332`: success |
| OBS / protocol, sandbox, real probe and fuzz | PASS | Observed run `36394993299`: success |
| Driver continuity | PASS | Observed run `36394993500`: success |
| Plasma Wayland / Openbox EWMH | PASS | Observed runs `36394993404` / `36394993501`: success |

The development matrix uses Rust 1.98.1, while Rust **1.88.0 is the declared and executed MSRV**. The hosted MSRV job runs the required fmt/check/build/Clippy/tests/doctests/docs/release/fake/federation gates at that lower bound. The normal x86_64/ARM64 matrix runs the locked workspace on 1.98.1. Source contracts run Python discovery, Node tests, source/schema validation and the native C/openat2 harness; a separate hosted static-lints job executes pinned Ruff 0.13.2, ShellCheck and actionlint including embedded workflow shell.

The dependency job runs `cargo audit --deny warnings` and `cargo deny --locked check`. Coverage
produces workspace LCOV and JSON artifacts; no percentage is asserted here. The fuzz job executes
the target list in `.github/workflows/security.yml` for bounded intervals (24 targets at
the preflight observation, including workflow, video, Figma, Godot and Skills surfaces). Workflows
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

The quality suite includes `crates/core/tests/provider_runtime.rs` and the dynamic provider
catalog tests. Their executed counts must be taken from the selected SHA's job log. These include simultaneous registration, operation-level availability,
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

This is regression evidence for the mediated federation path, not certification of the upstream
executable. The current launcher stages verified bytes and requires the platform sandbox;
`ExternalMcpProvider::sandbox_spec` fails closed when isolation is unavailable. Filesystem/network
grants are explicit. This does not protect against a separate malicious same-UID process outside
that sandbox, nor prove the kernel or native-code boundary correct. Remote
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
executes the broker smoke path and compiles a newly scaffolded driver. Driver Protocol v2 now
negotiates dynamic capabilities, provider events, progress/artifacts and cooperative cancellation;
the sandboxed `protocol_v2` fixture exercises event delivery, catalog change, typed artifact
metadata, monotonic progress and cancellation while v1 remains the compatibility baseline.

## Native SDK closure included in this development line

The canonical Native SDK is now part of `main` under `crates/native-sdk`, with an executable
TypeScript binding under `sdk/native-typescript` and reference integrations under `examples/native`
and `examples/native-inventory`. It is designed for applications that keep their own domain model,
persistence, revisions and transaction boundaries while exposing small optional cooperation
interfaces to Semwright.

The base SDK does not impose the file-backed reference model. Scene/Table/Counter remain an
optional compatibility/profile example, while the Inventory example owns a SQLite schema and its
own transactions. Operation contracts describe the guarantees an application actually provides
(commit point, retry/idempotency, undo, cancellation and revision CAS) rather than treating every
mutation as reversible or safely retryable. Recovery, pagination/events, snapshots/workspaces and
private publication are optional capabilities, not universal requirements.

The SDK adapts those application-owned interfaces through the existing Driver SDK and Driver Host.
It does not create another Broker, permission store, scheduler, Project Graph or Effect Conformance
authority. The real Host acceptance route is:

```text
CLI/MCP -> daemon -> Broker/Policy -> Driver Host -> native application
        -> persistence/artifacts -> admission/readback -> Project Graph/Effect Conformance
```

Public exact-SHA evidence covers repository contracts, the optional file-backed profile,
Driver/Graph contracts, executable TypeScript binding, clean external consumers, clean-room package
consumption and the Linux real-Host path. Full-portability run
[37179820287](https://github.com/seradotcom/semwright/actions/runs/37179820287) passed Ubuntu
x64/ARM64, Windows x64/ARM64 and macOS arm64/x64. Runs
[37181039129](https://github.com/seradotcom/semwright/actions/runs/37181039129) and
[37181039113](https://github.com/seradotcom/semwright/actions/runs/37181039113) passed the final
public integration head `09f71d490ac86f8f8e86dcda6c2552f50c59d487`, including the real Host route
above.

The Native SDK workflows now run for affected pull requests and `main` pushes. Evidence remains
exact-SHA scoped: a later source change must pass the affected Native SDK lanes rather than inherit
an earlier result. See [Native SDK overview](docs/native-sdk/README.md),
[verification](docs/native-sdk/VERIFY.md), and
[compatibility](docs/native-sdk/COMPATIBILITY.md).

## Windows platform closure and evidence boundary

Windows is an implemented native host, not merely a cross-compilation target. The platform contains
UI Automation semantics, owner-only Named Pipes, native filesystem confinement, secure
AppContainer/LPAC child spawn, Job Object containment, Windows.Graphics.Capture plumbing and
platform-owned authority profiles for supported Driver/Plugin/governed stdio MCP execution.

On source `04cf0ef7062d134b71206d832375be9545553bb4`, public run
[37228279724](https://github.com/seradotcom/semwright/actions/runs/37228279724) passed native x64,
native ARM64, `server2022-x64` sealed-tool compatibility and the
`win11-arm-native-host-x64tool` compatibility job. Historical Windows failures remain useful
evidence of what was corrected; they are not the current support statement.

Hosted Windows CI is deliberately **not** labeled interactive desktop certification. Foreground
UIA/input behavior, picker consent, UIPI/UAC, lock/wake, mixed-DPI/multi-monitor and
real-application cases require the unlocked-desktop procedure in
[`docs/windows/LIVE_WINDOWS_TEST_MATRIX.md`](docs/windows/LIVE_WINDOWS_TEST_MATRIX.md). External
MCP filesystem mounts also remain fail-closed under `BLOCKED_PORTABLE_PATH_VIRTUALIZATION` where
transparent path virtualization is not proven.

See [`docs/windows/README.md`](docs/windows/README.md),
[`docs/windows/WINDOWS_VERIFY.md`](docs/windows/WINDOWS_VERIFY.md), and
[platform support](docs/platforms.md) for the detailed platform boundary.

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
document and an executable payload. The current format also supports explicitly declared,
bounded companion files; their destinations and sizes are validated rather than treated as an
arbitrary archive. Platform executable admission remains distinct from package inspection. The package and executable are SHA-256 pinned; the index independently pins package
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

LibreOffice is a first-party deep application driver built on the public App Driver SDK. The
owner-pinned driver runs persistently
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

Provider Runtime now has a general bounded progress/artifact contract: provider signals update the
owning session-scoped JobStore, artifacts retain typed metadata, and integration tests prove
cross-session isolation. Driver Protocol v2 negotiates child events, progress/artifacts, dynamic
capabilities and cooperative cancellation, while the inspector exposes session Jobs/Refs views.
Remote durable task persistence remains follow-on work rather than an implied capability.

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

Driver Protocol v2 now provides the generic negotiated path for driver-child events, dynamic
capability changes, progress/artifacts and cooperative cancellation. That generic transport does
not by itself claim complete OBS event forwarding or convert every OBS-specific lifecycle signal
into a broker-native event; those application-level mappings remain separate from protocol closure.

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
not the optional GJS bridge or focused portal input dispatch. Separate real portal evidence below
covers consent and `ConnectToEIS` negotiation, while hosted jobs certify Plasma/KWin Wayland, real
headless Sway IPC and Openbox/EWMH X11. A separate owner-hardware run certifies Hyprland 0.56.2
nested on KWin 6.7.5 over a real AMD render node: the production native-socket backend lists,
focuses, moves and resizes a native Wayland fixture, rejects its stale ref after exit, cleans it up,
and verifies a synthetic second output at scale 1.25. Sanitized evidence is stored in
`verification/live-hyprland-kwin/summary.json`. Physical Hyprland login/restart and broader physical
mixed-scale coverage remain part of the live matrix.

The RemoteDesktop EIS sender is implemented in the platformized Linux host and a real EIS protocol
fixture negotiates a sender session and transmits keysym, UTF-8 text, relative pointer motion,
buttons and scrolling. Separate real GNOME Shell 46.0 Wayland evidence records an owner-approved
RemoteDesktop request for keyboard+pointer, successful `ConnectToEIS` negotiation with three EIS
devices and `input_route=eis`. A later owner-approved run targeted a disposable GTK4 Wayland window:
the production broker enforced a focused GNOME window ref, EIS `pointer.move(+24,+13)` was observed
by GTK as the exact same relative logical delta, a left click was observed by GTK, and moving focus
to another disposable target caused a fail-closed `Conflict` with no additional GTK pointer event.
Explicit `portal.stop` then produced `eis=inactive` and `session_active=false`. The sanitized pointer
evidence remains in `verification/live-portal-eis/gnome-connect-to-eis.json`.

Keyboard evidence has a stricter boundary. GNOME exposes keycode-only `ei_keyboard` rather than
`ei_text`; EIS-advertised XKB translation, cooperative in-flight cancellation, sender backpressure,
pacing and bidirectional modifier-feedback handling now have deterministic protocol coverage.
However, earlier non-isolated keyboard runs demonstrated that semantic focus checks alone do not prove
target exclusivity. A second attempt using GNOME Shell nested with separate HOME, runtime, D-Bus
and Wayland namespaces still shared graphical authority with the outer session. Those keyboard runs are diagnostic only and must not be used as live
target-delivery certification. The invalidated record is
`verification/live-portal-eis/gnome-eis-cancellation.json`, and the methodology boundary is recorded
in `verification/live-portal-eis/keyboard-targeting-methodology.json`. Future keyboard live testing
must use a VM or independent seat/session with an independently isolated graphical authority boundary.

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

This closes Semwright's `release_packaging_validation` gate and the development evidence gap for native tar/deb packaging, reproducibility and user install/uninstall. The separate `Supply-chain certification` workflow now evaluates the pinned Nix derivation, generates normalized reproducible CycloneDX SBOMs for the release binaries, builds x86_64/aarch64 certification bundles and emits GitHub artifact/SBOM attestations with scoped OIDC permissions. This does not claim universal publisher identity or platform notarization, and `release-readiness.json` remains fail-closed for the remaining live/security gates.

## Six-platform distribution closure included in this development line

The V1 distribution workflow produces the current candidate package set for Linux x86_64/aarch64,
Windows x86_64/ARM64 and macOS arm64/x86_64. The retained exact-main revalidation record
`verification/v1-engineering-closeout-revalidation.json` binds source
`9954c1f95f68305f32f153fe5ab302441845b7ed` to Actions run `37146331051`, where all six
platform jobs and the manifest job succeeded. The record preserves the PR-head/merge distinction,
package count and manifest digest instead of treating a synthetic PR merge as the source head.

This multiplatform distribution evidence does not close R06/R18 interactive environment cases,
satisfy the independent security-review release gate, or authorize publication by itself.

## Verification hardening included in the baseline

- The command schema contract derives its expected descriptors from the checked-in catalog
  (142 builtin descriptors at the preflight observation), rather than a stale fixed count.
- The local runner bounds time and output, records real exit codes and hashes, persists transitions,
  rejects contradictory PASS reports, and does not overwrite prior evidence.
- Release admission has an independent required-gate set and rejects malformed/partial metadata,
  non-boolean gates, floating toolchains, symlinks, and invalid lockfiles.
- The verifier is a trusted-tool process controller, not a sandbox for hostile plugins.
- The development checkout remains intentionally blocked by `release-readiness.json`; green CI is
  necessary but does not itself authorize a release.

## Repository governance observation

The retained R16 findings ledger records R-009 as an open governance decision. Repository API
re-observation on 2026-10-04 found no GitHub branch-protection rule for `main` and no repository
rulesets. This is not a runtime vulnerability or evidence of an unauthorized change; it means
review/status requirements are not enforced by those repository settings. See
[R16 findings](verification/r16-closeout/FINDINGS.json).

## Evidence boundaries

Historical website demonstrations remain exact-SHA observations: Figma Desktop
126.5.6 at 3cd86958f70f7a8231d31e492b5505acff19dae0 exercises the driver/Plugin API route,
while Godot Parcel Lantern at 9ecf35fd9c3d6fbcbc1f8b72b8d4734c70037ffa records a bounded
broker route and clean restart. These historical observations are not inherited by current main.

The observed workflows provide hosted regression evidence for their listed source SHAs. The
older Windows ARM64 fixture failure remains historical evidence, while run `37228279724` on source
`04cf0ef7062d134b71206d832375be9545553bb4` passed both native Windows architectures and both
sealed-tool compatibility jobs. Historical records separately cover GNOME semantic
GTK, nested Hyprland and isolated GNOME/Plasma VM input delivery/cancellation. In particular,
`verification/live-portal-eis/gnome-vm-keyboard-2026-09-26.json` and
`verification/live-portal-eis/plasma-kde-portal-notify-vm-2026-09-26.json` record the later isolated
keyboard paths; the earlier shared-authority keyboard attempts remain invalidated diagnostics.
These records do not constitute physical Hyprland-login or physical mixed-scale/multi-monitor
acceptance. The exact residual R06 conditions remain in `RELEASE_BLOCKERS.md`.

Linux hostile plugin/driver/federation prechecks ran with positive test counts at the observed
SHA. This is not an independent review, a formal sandbox proof, or an interchangeable Windows /
macOS / Linux security certificate. A remote signed marketplace, universal publisher identity,
full native application APIs and interactive Mac/Windows acceptance are not implied by hosted
success. R16 is now CLOSED after the separately recorded closeout revalidation;
`release-readiness.json` remains fail-closed because broader physical/interactive gates are separate.

Local exploratory evidence and `dummy-docs/` are intentionally excluded from Git. Historical
failed logs remain useful diagnostics but do not contribute to the accepted baseline. See
[ACCEPTANCE.md](ACCEPTANCE.md) and [RELEASE_BLOCKERS.md](RELEASE_BLOCKERS.md) for the remaining
scope.
