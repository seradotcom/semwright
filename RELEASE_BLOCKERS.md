# Release blockers — 0.9.0-dev.1

**This handoff does not meet the requested v1.0-grade completion checklist.** It contains
substantial implementation source and executed component checks, not a validated binary
release. The following issues must not be relabelled “live verification pending” when
implementation or compilation work is actually missing.

| ID | Blocker | Completion evidence needed |
|---|---|---|
| R01 | No Rust toolchain was available; workspace, tests and SDK integrations were not compiled | Clean-checkout `check`, `test`, `clippy`, `fmt`, `doc`, release build logs |
| R02 | No `Cargo.lock`, toolchain is floating `stable`, full dependency licenses/advisories unknown | Reviewed lockfile, pinned actual toolchain, audit/deny reports, MSRV decision |
| R03 | Rust broker/CLI/MCP/plugin integration is unexecuted | Fake-daemon end-to-end run, real MCP client negotiation/cancellation, malformed transport tests |
| R04 | EIS/libei transport, PipeWire pixel-stream decoding, portal restore-token persistence and clipboard-session integration are not implemented | Real paths, lifecycle/cancellation/coordinate contracts, consent revocation tests |
| R05 | AT-SPI delta snapshots and complete event-driven cache/conformance coverage are incomplete | Private D-Bus fixture + GTK/Qt live runs, event loss and object reuse regression tests |
| R06 | Desktop bridges/backends have not run on GNOME, Plasma, Sway, Hyprland or a native X11 desktop | Recorded desktop/version/session matrix, negative tests, focus drift and cancellation |
| R07 | X11 uses synchronous calls in async methods and fingerprints do not include a lifecycle event epoch | Bounded worker/process boundary, create/destroy/reuse tracking, unresponsive server tests |
| R08 | Blender Python was only tested with mocks; Rust bridge is unexecuted; path-based bpy I/O cannot be made FD-relative | Real background/GUI API tests; trusted workspace threat review and race documentation |
| R09 | Chromium Rust adapter is not validated; download quota enforcement and complete crash-profile/artifact cleanup are incomplete | Rust adapter against a disposable browser, multi-frame/event races, download limits and crash cleanup |
| R10 | Plugin sandbox was not executed; runtime handshake verifies protocol/name, not a full independent schema/version digest | Negative filesystem/network/process/env tests, updated handshake attestation and watchdog tests |
| R11 | Many output schemas are generic objects; planner uses backend-level probe availability rather than every operation-specific state | Tighten output contracts and operation capability probing with compatibility fixtures |
| R12 | Schema/recipe evolution, richer inspector ref workflows, service CLI, observation event breadth and long-task MCP mapping are incomplete | Versioned migration tests and complete UX contracts, or explicitly reduced release scope |
| R13 | CI definitions, architecture builds, fuzzing, Rust coverage and benchmarks were not executed | Hosted CI evidence; bounded fuzz logs, honest whole-workspace coverage, measured benchmarks |
| R14 | Binary tarballs, `.deb`, aarch64 builds and Nix expression are configured only; no reproducible release/publishing/SBOM/signing | Built/installed/uninstalled artifacts, immutable Actions/dependencies, provenance and release ownership |
| R15 | Core/application security has not had an independent review | Peer review of authorization, prompt injection containment, cancellation, stale identity and disclosure boundaries |

Additional limitations: recipe output redaction is a best-effort taint mechanism, not
formal information-flow security; some failure paths do not yet include full progress
metadata. Plugin installation through IPC is session-persistent, not a durable trust-store
UI. Runtime capability discovery is not a certification of support. An app-native command
addressed by an object name does not inherit the desktop ref store's generation semantics.

The acceptance table records every original checklist entry. Do not remove requirements
or rename failing checks to produce an artificial “all PASS”. `release-readiness.json`
and `scripts/release/assert-ready.py` prevent accidental binary-release admission.
