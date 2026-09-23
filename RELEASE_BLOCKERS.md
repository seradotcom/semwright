# Release blockers — 0.9.0-dev.1

**Baseline for all statements below: the exact Git commit containing this document.**

The hosted source, Rust, dependency, coverage, bounded-fuzz, fake-E2E, real Chromium, sandboxed
driver-conformance and real LibreOffice/UNO jobs are green on the certified development line.
Provider Runtime, governed stdio MCP federation, the persistent App Driver SDK and a second deep
application driver have executed integration evidence. Those development gates are closed; this
is still not a release candidate.

| ID | Remaining blocker | Completion evidence needed |
|---|---|---|
| R01 | No MSRV policy or compatibility range has been established; Rust 1.98.1 is the tested pin, not an MSRV. | Document a supported range and execute its lower bound. |
| R02 | EIS/libei sender transport and direct protocol lifecycle are implemented, and a real GNOME/Wayland host exposes RemoteDesktop v2 `ConnectToEIS`; portal-granted execution is not yet accepted. | Real user-consented portal→EIS session, revocation/cancellation and coordinate/lifecycle evidence. |
| R03 | PipeWire ScreenCast pixel decoding and robust stream lifecycle are incomplete. | Real frames, format negotiation, damage/resize, cancellation and resource cleanup tests. |
| R04 | Portal restore-token persistence and clipboard/session integration are incomplete. | Durable scoped storage plus consent/revocation and stale-token tests. |
| R05 | AT-SPI delta snapshots, event-loss recovery and object-reuse coverage are incomplete. | Private D-Bus fixtures and GTK/Qt live conformance. |
| R06 | GNOME, Plasma, Sway, Hyprland and native X11 live matrices are unexecuted. | Versioned session matrix with negative, focus-drift and cancellation tests. |
| R07 | X11 still needs a bounded blocking boundary and lifecycle epochs. | Unresponsive-server tests and create/destroy/reuse tracking. |
| R08 | Blender has mocked Python coverage but no accepted real Blender/RNA/addon execution. | Background and GUI Blender runs, introspection/addon discovery, refs, render/export and cleanup. |
| R09 | Chromium now has a real Rust happy/negative integration, but quotas, crash recovery, multi-frame races and artifact lifecycle need deeper coverage. | Quota and crash matrices with deterministic cleanup and stale-ref tests. |
| R10 | App Driver conformance and real LibreOffice execute inside the sandbox with bounded resources, but hostile plugin/driver escape coverage and plugin handshake attestation remain incomplete. | Filesystem/network/process/env escape tests, plugin schema/version digest attestation and watchdog/adversarial sandbox tests. |
| R13 | Core event provenance, session-private job lifecycle, revocation and cancellation are implemented and integration-tested; provider progress/artifacts, MCP task mapping, negotiated driver job/event interfaces and richer inspector workflows remain incomplete. | Cross-provider progress/artifact contracts, MCP/driver task conformance and inspector/reference workflow evidence. |
| R14 | Several outputs and availability signals remain too generic or backend-wide. | Tight output schemas and operation-specific probing/compatibility fixtures. |
| R15 | Binary packages, install/uninstall, Nix evaluation, reproducibility, SBOM/signing and publishing provenance are unverified. | Clean hosted artifact matrix and installation/removal evidence. |
| R16 | Live desktop/application security has no independent review. | Peer review of policy, prompt-injection containment, cancellation, stale identity and disclosure boundaries. |

Closed development blocker: **R11** (governed local stdio MCP federation) now has real upstream
fixture sessions, namespaced untrusted capability import, central policy mediation, cancellation,
dynamic catalog refresh, crash invalidation and owner-registry tests. Same-UID upstream process
sandboxing remains explicitly open under the security boundary rather than being hidden by R11.

Closed development blocker: **R12** (driver registry/distribution) now has a bounded static/local
index, Semwright/application compatibility resolution and a non-archival `.swdp` package format
containing one strict metadata document plus one SHA-256-pinned ELF. The hosted `driver-distribution`
job executes package/index validation plus install/update/remove smoke tests and proves installation
does not execute the payload or create policy grants. This closes the specified local/static
distribution gate; remote catalog transport, a hosted marketplace and cryptographic publisher
identity/signatures are not implied.

Closed development milestone: the App Driver SDK now has a real non-browser/non-Blender showcase.
LibreOffice Writer/Calc/PDF operations execute through the normal broker and a persistent sandboxed
DriverProvider. This closes the SDK generalization demonstration, not R10's hostile sandbox matrix,
remote/signed driver publishing, or any claim of complete UNO application coverage.

Additional limitations remain: recipe taint/redaction is not formal information-flow security;
runtime discovery is not certification; application names do not automatically inherit desktop-ref
generation semantics. `release-readiness.json` must remain blocked until the corresponding gates are
actually evidenced, not merely implemented or documented.
