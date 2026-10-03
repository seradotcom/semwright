# Release blockers — 0.9.0-dev.1

**This is a historical blocker ledger, not evidence that every gate ran on its containing commit.**

The R16 review snapshot is `6491c0d838fa066938a494524d69ed507aa0dbe8`. Integrated engineering
certificates retain source `cd518748f742025a251b78028613aa1b16919e73`, their suite SHAs and
explicit job dispositions. Native Windows run `37096430846` passed on the review snapshot;
that does not make skipped main-push jobs executed or close interactive R18.
See [integrated evidence](docs/semantic-creation/INTEGRATION.md), [verification](VERIFY.md)
and [R's evidence directory](verification/r16-closeout/README.md).

The closure notes below retain their recorded historical scope. R16 is CLOSED after separate
revalidation; R06 and R18 remain OPEN — DEFERRED_TO_POST_V1_ENVIRONMENT_DEPENDENT.
`release-readiness.json` is unchanged and remains fail-closed. This remains development software.

| ID | Remaining blocker | Completion evidence needed |
|---|---|---|
| R06 | Cross-desktop live evidence now includes GNOME Wayland, Plasma/KWin Wayland, headless Sway, Openbox/EWMH X11 and a hardware-backed nested Hyprland 0.56.2 run on a real AMD render node. The Hyprland run certifies native socket discovery/focus/move/resize, stale-reference rejection, cleanup and a synthetic second output at scale 1.25. A real owner GNOME Wayland login additionally certifies bridge disable/enable reconnect: capability moved from SUPPORTED to UNAVAILABLE while the extension was absent and recovered to backend `gnome` within about 62 ms after re-enable. An isolated real-login Plasma/KWin 5.27 VM certifies a full `plasma-kwin_wayland.service` restart: the compositor PID changed and Semwright automatically recovered backend `kwin` in about 246 ms without a manual post-restart script reload. An independent KVM GNOME 46 real-login guest now also certifies a full GDM/graphical-session restart through the production systemd-user lifecycle: graphical session 22→59, GNOME Shell PID 3027→10009 and Semwright service PID 8909→10281, followed by automatic broker/bridge ownership recovery and a successful `window.list` through backend `gnome` against a new Wayland fixture. Plasma focus-drift and in-flight cancellation are covered by the isolated portal keyboard evidence. The physical host had only one connected eDP panel; HDMI/DP were disconnected, so physical mixed-scale/multi-monitor could not be exercised without inventing hardware. | Execute only the remaining physical cases that require conditions not presently available without disrupting the owner: a physical Hyprland login, and physical mixed-scale/multi-monitor when a second display is actually connected. |
| R16 | **CLOSED.** A separate reviewer session inspected/adopted the R-authored bounded MCP pagination remediation and R16 assurance wording. This is not represented as an external organizational audit. | No remaining R16 action. Preserve `verification/r16-closeout/evidence/INDEPENDENT_R16_REVALIDATION_2026-10-03.json`; broader platform/release gates remain separate. |
| R18 | Windows native CI now proves secure spawn and the implemented authority profiles, but no exact-commit unlocked-desktop certification bundle exists yet. External MCP filesystem mounts also remain deliberately fail-closed because Windows lacks a proven transparent `/workspace/<name>` path-virtualization contract for third-party MCPs. | Run `scripts/windows/run-interactive-certification.ps1` (or the self-hosted `windows-interactive.yml` workflow) on a disposable unlocked Windows desktop; attach the evidence bundle; complete the remaining UIPI/UAC, mixed-DPI/multi-monitor, session lifecycle, UIA virtualization/events and real-app rows. Keep external MCP mounts blocked unless a separately reviewed portable virtualization design is proven. |

Closed development blocker **R02**: portal-granted keyboard control now has target-delivery evidence on two supported Wayland desktops inside isolated authority boundaries. GNOME Shell 46 uses `ConnectToEIS`; an independent KVM GNOME guest delivered 4096/4096 uppercase characters in one call, demonstrated cooperative in-flight cancellation with no post-result growth or stuck Shift, and stopped to an inactive EIS state. Plasma/KWin 5.27.11 on Ubuntu 24.04.5 exposes RemoteDesktop v2 but not `ConnectToEIS`, so Semwright correctly uses the explicit `portal_notify` fallback. With sustained pacing at four characters per 8 ms, the exact candidate commit delivered 4096/4096 uppercase characters in 9.595 s, cancellation after about 20 ms settled at one character with a successful lowercase follow-up, focus drift was rejected with `Conflict` and no fallback or target mutation, and explicit stop left the portal inactive. Evidence: `verification/live-portal-eis/gnome-vm-keyboard-2026-09-26.json` and `verification/live-portal-eis/plasma-kde-portal-notify-vm-2026-09-26.json`. Non-isolated direct-login and shared-authority nested-shell keyboard methods remain invalidated. Broader physical/real-login, restart and mixed-scale coverage remains R06 rather than being relabeled as R02.

Closed development blocker **R17**: Ubuntu 24.04/Noble `at-spi2-core 2.52.0` can terminate GNOME Shell under heavy AT-SPI automation through the upstream SpiCache lifetime bug tracked by Ubuntu #2158636. Semwright now carries a conservative runtime guard plus an exact Noble backport of upstream `d442ee18`. Hosted CI builds/verifies the local `.deb`, executes 12 patched AT-SPI lifecycle iterations, and compiles/tests the guard. A controlled real Noble GNOME host then loaded `2.52.0-1build1+semwright1` and executed direct AT-SPI enumeration/churn, Semwright normal and guarded snapshots, repeated event-driven churn, stale-ref recovery, and a semantic `ui.invoke` mutation without reproducing the historical SIGSEGV. Evidence: `verification/live-gnome/noble-atspi-backport-2026-09-25.json`. This closes the development blocker for the tested compatibility path, not universal certification of all Noble hardware/session combinations.

Closed development blocker **R01**: Rust 1.88 is the declared workspace MSRV and the hosted MSRV job executes the required fmt/check/build/Clippy/tests/doctests/docs/release/fake/federation gate set. Rust 1.98.1 remains the development pin rather than being mislabeled as the minimum.

Closed development blocker **R09**: the real Rust Chromium matrix now exercises bounded per-file/count/total download quotas, CDP cancellation, crash/dead-instance relaunch, screenshot/download artifact lifecycle, stale refs and real multi-frame navigation. These tests retain disposable profiles and do not expand browser authority.

Supply-chain closure under **R15** now extends the existing native packaging evidence: `Supply-chain certification` evaluates the pinned Nix derivation, generates normalized reproducible CycloneDX SBOMs, builds x86_64/aarch64 certification bundles and emits GitHub artifact/SBOM attestations. `Packaging certification` separately retains normalized tar/deb reproducibility plus private install/execute/uninstall and tamper-safe removal evidence.

Live-matrix progress under **R06**: GNOME Shell 46.0 on a real Wayland login session executes the production AT-SPI backend against a disposable Zenity/GTK fixture, and a separate owner-approved GNOME portal run negotiates keyboard+pointer `ConnectToEIS` and explicit session shutdown. Hosted `Plasma Wayland live` executes the KWin 6 mailbox bridge through discovery/focus/resize/move/close/stale-ref lifecycle; the Sway fixture executes the native IPC path on a real headless compositor; and `Native X11 EWMH live` executes the X11 backend against Openbox. Owner-hardware certification now additionally runs Hyprland 0.56.2/Aquamarine 0.15.1 nested on KWin 6.7.5 over a real AMD render node, exercises Semwright native-socket focus/move/resize/stale-ref/cleanup, and verifies a synthetic second output at scale 1.25. Physical Hyprland login/restart, focus-drift/cancellation and broader physical mixed-scale/real-login coverage remain outside this closure.

Closed development blocker **R03**: the platformized Linux host now implements bounded XDG
ScreenCast + PipeWire capture. Hosted native integration creates a real synthetic PipeWire source,
negotiates a stream, copies bounded raw frames, validates stride/format handling and writes private
PNG artifacts. This does not substitute for the broader live desktop matrix in R06.

Closed development blocker **R04**: owner-private RemoteDesktop restore-token state now supports
process and durable modes, token rotation/single-use handling and explicit clearing. Clipboard
read/write is integrated into the consented RemoteDesktop session. Private D-Bus fixtures execute
restore rotation and clipboard grant lifecycle without exposing token material. A real GNOME portal
input consent/`ConnectToEIS` lifecycle is now executed separately; focused input and the broader
portal/desktop matrix remain tracked by R02/R06 rather than being relabeled as complete coverage.

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

Closed development blocker **R13**: Provider Runtime progress and artifact signals feed the owning
session-scoped JobStore, `jobs.list` and inspector Jobs/Refs views are implemented, MCP Tasks map to
the broker job model, and Driver Protocol v2 negotiates dynamic capabilities, events, progress,
artifacts and cooperative cancellation. Hosted provider and sandboxed Driver Host conformance tests
exercise these contracts.

Closed development blocker **R14**: built-in capability output schemas were tightened against the
real fixture/catalog variants, including strict union deduplication and compatibility regressions;
the exact-commit quality/native matrices pass with the stricter schemas.

Closed development blocker **R15**: hosted supply-chain certification evaluates the pinned Nix
derivation, builds native x86_64/aarch64 certification bundles, generates normalized reproducible
CycloneDX SBOMs, and publishes GitHub artifact/SBOM attestations with scoped OIDC permissions.
Native tar/deb reproducibility and private install/execute/uninstall remain separately certified.
This is supply-chain provenance evidence, not an assertion that every distribution channel is signed.

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
