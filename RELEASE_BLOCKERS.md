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
| R02 | EIS/libei sender transport is implemented and protocol-tested, but a real user-approved portal ConnectToEIS session is not yet certified. | Live portal-granted sessions with consent, revocation, cancellation, coordinate/focus and lifecycle evidence on supported Wayland desktops. |
| R06 | The cross-desktop live matrix is incomplete. GNOME Shell 46.0 Wayland now has real semantic GTK/AT-SPI mutation/delta/stale-ref evidence; hosted X11 and GTK/Qt fixtures also execute, but Plasma Wayland, Sway, Hyprland and a native desktop X11 session are not all certified. | Versioned remaining-session matrix with negative cases, focus drift, scaling/multi-monitor where available, cancellation and cleanup. |
| R13 | Core event provenance, session-private jobs and MCP Tasks mapping are implemented; provider-wide progress/artifacts, negotiated driver-child events/cooperative cancellation and richer inspector workflows remain incomplete. | Cross-provider progress/artifact contracts, Driver Protocol task/event/cancellation conformance and inspector/reference workflow evidence. |
| R14 | Some outputs and availability signals remain broader/generic than the final semantic API should expose. | Tight output schemas and operation-specific probing/compatibility fixtures across representative providers. |
| R15 | Semwright binary packaging/install/uninstall, Nix evaluation, reproducibility, SBOM/signing and publishing provenance are not fully certified. | Clean hosted artifact matrix, installation/removal tests, reproducibility receipts and release provenance. |
| R16 | No independent security review has closed the remaining host/application attack surface. | Peer review of authorization, prompt-injection containment, cancellation, stale identity, sandbox boundaries and disclosure behavior. |

Closed development blocker **R01**: Rust 1.88 is the declared workspace MSRV and the hosted MSRV job executes the required fmt/check/build/Clippy/tests/doctests/docs/release/fake/federation gate set. Rust 1.98.1 remains the development pin rather than being mislabeled as the minimum.

Closed development blocker **R09**: the real Rust Chromium matrix now exercises bounded per-file/count/total download quotas, CDP cancellation, crash/dead-instance relaunch, screenshot/download artifact lifecycle, stale refs and real multi-frame navigation. These tests retain disposable profiles and do not expand browser authority.

Live-matrix progress under **R06**: GNOME Shell 46.0 on a real Wayland login session now executes the production AT-SPI backend against a disposable Zenity/GTK fixture. Discovery, full snapshot, semantic text mutation, delta refresh, application close/resync and stale-ref rejection pass on commit `6bab0cc`. This closes the GNOME/A025/A029 evidence slice only; it does not certify portal input consent, the optional GJS bridge, Plasma, Sway, Hyprland or native desktop X11.

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
