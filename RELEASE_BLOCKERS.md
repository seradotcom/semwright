# Release blockers — 0.9.0-dev.1

**Baseline for all statements below: the exact Git commit containing this document.**

The hosted source, Rust, dependency, coverage, bounded-fuzz, fake-E2E, and real Chromium jobs are
green on that exact commit. The explicit Provider Runtime foundation (identity/provenance, dynamic
catalog lifecycle, operation availability, disconnect/cancellation and bounded external schemas)
is also closed by executed integration tests. Those gates are closed; this is still not a release
candidate.

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
| R10 | Plugin sandbox and the future public Driver SDK conformance surface are not certified by executed hostile negative tests; plugin handshake attestation remains incomplete. | Filesystem/network/process/env escape tests, schema/version digest attestation, driver conformance and watchdog tests. |
| R11 | MCP federation is not implemented/validated end to end. | Real upstream sessions, namespacing, central policy, untrusted-content containment, cancellation and invalidation. |
| R12 | App Driver SDK/registry distribution, safe packages and compatibility resolution are incomplete. | Conformance harness, signed/checksummed safe extraction, no install-time execution and compatibility fixtures. |
| R13 | Events/jobs, long-operation progress/cancellation and richer inspector/reference workflows are incomplete. | Source-tagged event and structured job integration tests across drivers/backends. |
| R14 | Several outputs and availability signals remain too generic or backend-wide. | Tight output schemas and operation-specific probing/compatibility fixtures. |
| R15 | Binary packages, install/uninstall, Nix evaluation, reproducibility, SBOM/signing and publishing provenance are unverified. | Clean hosted artifact matrix and installation/removal evidence. |
| R16 | Live desktop/application security has no independent review. | Peer review of policy, prompt-injection containment, cancellation, stale identity and disclosure boundaries. |

Additional limitations remain: recipe taint/redaction is not formal information-flow security;
runtime discovery is not certification; application names do not automatically inherit desktop-ref
generation semantics. `release-readiness.json` must remain blocked until the corresponding gates are
actually evidenced, not merely implemented or documented.
