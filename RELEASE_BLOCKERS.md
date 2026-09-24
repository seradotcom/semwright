# Release blockers — 0.9.0-dev.1

**Baseline for all statements below: the exact Git commit containing this document.**

The hosted source, Rust, dependency, coverage, bounded-fuzz, fake-E2E, native application,
driver-distribution, X11, AT-SPI, PipeWire and platformization jobs are green on the certified
development line. Provider Runtime, governed stdio MCP federation, persistent App Driver SDK,
deep application drivers, EIS transport, AT-SPI delta recovery, X11 lifecycle hardening,
PipeWire frame capture and portal restore/clipboard persistence all have executed evidence.
This remains a development snapshot and is not a release candidate.

| ID | Remaining blocker | Completion evidence needed |
|---|---|---|
| R02 | A real GNOME Wayland RemoteDesktop session now reaches `ConnectToEIS`, negotiates keyboard + pointer devices and cleanly returns to `eis=inactive` after `portal.stop`; cancellation plus focus/coordinate behavior and the broader portal desktop matrix remain uncertified. | Live portal-granted cancellation and focus/coordinate evidence, plus the remaining supported-desktop consent/revocation matrix. |
| R06 | The cross-desktop live matrix is incomplete. GNOME Wayland, Plasma/KWin 6, Sway and WM-managed Openbox/X11 now have executed lifecycle evidence; Hyprland live execution and the broader scaling/multi-monitor/failure matrix remain incomplete. | Versioned Hyprland live evidence on a compatible compositor/DRM environment plus the remaining negative, focus-drift, scaling/multi-monitor, cancellation and cleanup cases required by release scope. |
| R15 | Nix evaluation, SBOM/signing and publication provenance remain uncertified. Native x86_64/ARM64 tar/deb packaging, reproducibility and user install/execute/uninstall are now certified. | Evaluate the Nix path and produce/review SBOM, signing and publication provenance without weakening fail-closed release admission. |
| R16 | No independent security review has closed the remaining host/application attack surface. | Peer review of authorization, prompt-injection containment, cancellation, stale identity, sandbox boundaries and disclosure behavior. |

Closed development blocker **R01**: Rust 1.88 is the declared workspace MSRV and the hosted MSRV job executes the required fmt/check/build/Clippy/tests/doctests/docs/release/fake/federation gate set. Rust 1.98.1 remains the development pin rather than being mislabeled as the minimum.

Closed development blocker **R09**: the real Rust Chromium matrix now exercises bounded per-file/count/total download quotas, CDP cancellation, crash/dead-instance relaunch, screenshot/download artifact lifecycle, stale refs and real multi-frame navigation. These tests retain disposable profiles and do not expand browser authority.

Closed development blocker **R13**: MCP Tasks map negotiated asynchronous tool execution onto the session-scoped JobStore, Driver Protocol v2 negotiates child events/progress/artifacts/cooperative cancellation with a sandboxed conformance fixture, dynamic-provider integration proves bounded progress/artifacts flow into the owning job, and `jobs.list` plus Jobs/Refs inspector panes expose the resulting state without bypassing broker policy. Remote durable jobs and `input_required` remain explicit non-claims rather than missing transport contracts.

Closed development blocker **R14**: every built-in top-level output schema is now an explicit closed contract, cross-platform results use explicit variants rather than arbitrary objects, and source-contract regression tests prevent generic outputs from returning. Provider Runtime operation-level availability and representative backend compatibility fixtures remain fail-closed when an operation is unavailable.

Packaging progress under **R15**: the hosted `Packaging certification` workflow builds the five release executables natively on x86_64 and ARM64, creates normalized tar/deb packages twice, compares hashes, validates payloads, and performs private user install/execute/uninstall including tamper-safe removal. This closes `release_packaging_validation`; Nix evaluation, SBOM/signing and publication provenance remain separate release blockers.

Live-matrix progress under **R06**: GNOME Shell 46.0 Wayland executes the production AT-SPI backend against a disposable GTK fixture with mutation/delta/stale-ref recovery. Hosted live gates additionally exercise KWin 6 on virtual Wayland, Sway 1.9 on a real headless wlroots compositor, and Openbox-managed EWMH behavior under Xvfb, including focus/move/resize/close and stale-reference lifecycle where supported. Hyprland remains the missing compositor certificate: its current Aquamarine Wayland backend requires dmabuf/DRM facilities unavailable in the hosted container used by the attempted live gate. This evidence also does not substitute for the broader scaling/multi-monitor and portal-consent matrix.

Closed development blocker **R03**: the platformized Linux host now implements bounded XDG
ScreenCast + PipeWire capture. Hosted native integration creates a real synthetic PipeWire source,
negotiates a stream, copies bounded raw frames, validates stride/format handling and writes private
PNG artifacts. This does not substitute for the broader live desktop matrix in R06.

Closed development blocker **R04**: owner-private RemoteDesktop restore-token state now supports
process and durable modes, token rotation/single-use handling and explicit clearing. Clipboard
read/write is integrated into the consented RemoteDesktop session. Private D-Bus fixtures execute
restore rotation and clipboard grant lifecycle without exposing token material. Real portal UI
consent remains part of R02/R06 rather than being relabeled as complete live coverage.

Closed development blocker **R05**: AT-SPI delta snapshots, structural resync, event-driven stale
reference recovery and object/app disappearance are implemented. Dedicated hosted GTK and native
Qt jobs execute disposable accessibility fixtures, real text mutation, delta refresh and stale-ref
recovery.

Closed development blocker **R07**: X11 synchronous work is isolated behind a bounded blocking
boundary with timeout/cancellation semantics, and window identity carries lifecycle epochs.
Hosted Xvfb integration exercises create/discover/destroy/reuse behavior.

Closed development blocker **R08**: both Blender integration shapes have real Blender 4.5.14
evidence. The sandboxed DriverProvider exercises RNA/operator/add-on introspection, object/material
mutation, render and .blend save; the interactive add-on path executes through the broker/CLI and
Blender main-thread timer under Xvfb.

Closed development blocker **R11**: governed local stdio MCP federation has real upstream sessions,
namespaced untrusted capability import, central policy mediation, cancellation, dynamic catalog
refresh, crash invalidation and owner-registry tests. Same-UID upstream sandboxing remains a
security-hardening concern rather than hidden by this closure.

Closed development blocker **R12**: static/local driver distribution uses a bounded .swdp package,
SHA-256-pinned ELF payload, compatibility resolution and safe install/update/remove without
install-time execution or implicit policy grants. Remote marketplace transport and cryptographic
publisher identity are explicitly outside this closure.

Closed development blocker **R10**: Plugin Protocol v2 mutually attests plugin name, version and
complete ordered command-descriptor SHA-256 before execution. Hosted hostile plugin and
DriverProvider fixtures execute through the production Bubblewrap + Landlock launcher and verify
read-only/write mount boundaries, host-file/PID/loopback isolation, scrubbed environments, driver
RLIMIT enforcement, watchdog/child cleanup and fail-closed descriptor/version mismatch handling.
This is executed regression evidence for the configured Linux sandbox boundary, not a formal proof
against kernel, Bubblewrap, Landlock or native-code vulnerabilities; independent review remains R16.

Closed development milestones also include real deep-driver evidence for Chromium, LibreOffice,
Blender, KiCad/MLT and OBS. These demonstrate Driver SDK generality; they do not imply complete
coverage of each application's native API.

Additional limitations remain: recipe taint/redaction is not formal information-flow security;
runtime discovery is not certification; application names do not automatically inherit desktop-ref
generation semantics. release-readiness.json remains fail-closed until every release gate is
actually evidenced.
