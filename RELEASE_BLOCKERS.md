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
| R01 | No MSRV policy or compatibility range has been established; Rust 1.98.1 is the tested pin, not an MSRV. | Document a supported lower bound and execute the full required gate set on it. |
| R02 | EIS/libei sender transport is implemented and protocol-tested, but a real user-approved portal ConnectToEIS session is not yet certified. | Live portal-granted sessions with consent, revocation, cancellation, coordinate/focus and lifecycle evidence on supported Wayland desktops. |
| R06 | The cross-desktop live matrix is incomplete. X11 executes under Xvfb and AT-SPI executes against real GTK/Qt fixtures, but GNOME Wayland, Plasma Wayland, Sway, Hyprland and a native desktop X11 session are not all certified. | Versioned session matrix with negative cases, focus drift, scaling/multi-monitor where available, cancellation and cleanup. |
| R09 | Chromium has substantial real Rust integration, but download quotas, crash recovery, multi-frame races and complete artifact lifecycle still need deeper coverage. | Quota/crash/frame-race matrices with deterministic cleanup, stale refs and bounded artifacts. |
| R10 | Driver sandboxing is executed with Bubblewrap + Landlock and scrubbed environments, but hostile plugin/driver escape coverage and stronger plugin handshake attestation remain incomplete. | Filesystem/network/process/env escape tests, schema/version digest attestation, watchdog behavior and adversarial sandbox regression jobs. |
| R13 | Core event provenance and session-private jobs are implemented; provider progress/artifacts, MCP task mapping, negotiated driver child events/cancellation and richer inspector workflows remain incomplete. | Cross-provider progress/artifact contracts, MCP/driver task conformance and inspector/reference workflow evidence. |
| R14 | Some outputs and availability signals remain broader/generic than the final semantic API should expose. | Tight output schemas and operation-specific probing/compatibility fixtures across representative providers. |
| R15 | Semwright binary packaging/install/uninstall, Nix evaluation, reproducibility, SBOM/signing and publishing provenance are not fully certified. | Clean hosted artifact matrix, installation/removal tests, reproducibility receipts and release provenance. |
| R16 | No independent security review has closed the remaining host/application attack surface. | Peer review of authorization, prompt-injection containment, cancellation, stale identity, sandbox boundaries and disclosure behavior. |

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

Closed development milestones also include real deep-driver evidence for Chromium, LibreOffice,
Blender, KiCad/MLT and OBS. These demonstrate Driver SDK generality; they do not imply complete
coverage of each application's native API.

Additional limitations remain: recipe taint/redaction is not formal information-flow security;
runtime discovery is not certification; application names do not automatically inherit desktop-ref
generation semantics. release-readiness.json remains fail-closed until every release gate is
actually evidenced.
