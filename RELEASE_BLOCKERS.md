# Release blockers — 0.9.0-dev.1

**Baseline for all statements below: the exact Git commit containing this document.**

The hosted source, Rust, dependency, coverage, bounded-fuzz, fake-E2E, real Chromium and
sandboxed driver-conformance jobs are green on the certified development line. Provider Runtime,
governed stdio MCP federation and the persistent App Driver SDK foundation have executed
integration evidence. Those foundation gates are closed; this is still not a release candidate.

| ID | Remaining blocker | Completion evidence needed |
|---|---|---|
| R01 | No MSRV policy or compatibility range has been established; Rust 1.98.1 is the tested pin, not an MSRV. | Document a supported range and execute its lower bound. |
| R02 | EIS/libei input transport is incomplete. | Real portal-granted sessions, revocation, cancellation, coordinate and lifecycle tests. |
| R03 | PipeWire ScreenCast pixel decoding and robust stream lifecycle are incomplete. | Real frames, format negotiation, damage/resize, cancellation and resource cleanup tests. |
| R04 | Portal restore-token persistence and clipboard/session integration are incomplete. | Durable scoped storage plus consent/revocation and stale-token tests. |
| R05 | AT-SPI delta snapshots, event-loss recovery and object-reuse coverage are incomplete. | Private D-Bus fixtures and GTK/Qt live conformance. |
| R06 | GNOME, Plasma, Sway, Hyprland and native X11 live matrices are unexecuted. | Versioned session matrix with negative, focus-drift and cancellation tests. |
| R07 | X11 still needs a bounded blocking boundary and lifecycle epochs. | Unresponsive-server tests and create/destroy/reuse tracking. |
| R08 | Blender has mocked Python coverage but no accepted real Blender/RNA/addon execution. | Background and GUI Blender runs, introspection/addon discovery, refs, render/export and cleanup. |
| R09 | Chromium now has a real Rust happy/negative integration, but quotas, crash recovery, multi-frame races and artifact lifecycle need deeper coverage. | Quota and crash matrices with deterministic cleanup and stale-ref tests. |
| R10 | The App Driver happy-path sandbox/conformance is executed, but hostile plugin/driver sandbox escape coverage and plugin handshake attestation remain incomplete. | Filesystem/network/process/env escape tests, plugin schema/version digest attestation and watchdog/adversarial sandbox tests. |
| R12 | Driver registry/distribution, safe install/update packages and application-version compatibility resolution are incomplete. | Checksummed safe extraction, no install-time execution, compatibility fixtures and a static/local index implementation. |
| R13 | Events/jobs, long-operation progress/cancellation and richer inspector/reference workflows are incomplete. | Source-tagged event and structured job integration tests across drivers/backends. |
| R14 | Several outputs and availability signals remain too generic or backend-wide. | Tight output schemas and operation-specific probing/compatibility fixtures. |
| R15 | Binary packages, install/uninstall, Nix evaluation, reproducibility, SBOM/signing and publishing provenance are unverified. | Clean hosted artifact matrix and installation/removal evidence. |
| R16 | Live desktop/application security has no independent review. | Peer review of policy, prompt-injection containment, cancellation, stale identity and disclosure boundaries. |

Closed development blocker: **R11** (governed local stdio MCP federation) now has real upstream
fixture sessions, namespaced untrusted capability import, central policy mediation, cancellation,
dynamic catalog refresh, crash invalidation and owner-registry tests. Same-UID upstream process
sandboxing remains explicitly open under the security boundary rather than being hidden by R11.

Additional limitations remain: recipe taint/redaction is not formal information-flow security;
runtime discovery is not certification; application names do not automatically inherit desktop-ref
generation semantics. `release-readiness.json` must remain blocked until the corresponding gates are
actually evidenced, not merely implemented or documented.
