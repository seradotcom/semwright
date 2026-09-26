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
| R02 | A real owner-approved GNOME Wayland RemoteDesktop session now certifies portal consent, keyboard+pointer `ConnectToEIS` negotiation, explicit stop and inactive post-stop state. Focused input dispatch, coordinate behavior and cancellation of an in-flight input operation remain uncertified. | Execute focused pointer/keyboard operations against a disposable target, verify focus-drift rejection and coordinate behavior, cancel an in-flight input request cleanly, and extend portal-granted evidence to additional supported Wayland desktops where feasible. |
| R06 | The cross-desktop live matrix is incomplete. GNOME Wayland, Plasma/KWin Wayland, headless Sway and Openbox/EWMH X11 now have executed semantic/native evidence; Hyprland live still cannot be certified on the hosted container because Aquamarine requires a dmabuf-capable parent/DRM path unavailable there, and broader real-login/scaling/multi-monitor coverage remains. | Versioned remaining-session matrix with Hyprland on a suitable hardware/session plus negative cases, focus drift, scaling/multi-monitor where available, cancellation and cleanup. |
| R16 | No independent security review has closed the remaining host/application attack surface. | Peer review of authorization, prompt-injection containment, cancellation, stale identity, sandbox boundaries and disclosure behavior. |
| R17 | Ubuntu 24.04/Noble `at-spi2-core 2.52.0` can terminate GNOME Shell under heavy AT-SPI automation via the upstream SpiCache lifetime bug tracked by Ubuntu #2158636. Semwright now carries a conservative runtime guard and an exact Noble backport of upstream `d442ee18`, but the patched real-session matrix is not yet certified. | Build and verify the backport reproducibly, pass disposable pressure tests, install it on a controlled Noble host, then execute repeated GNOME semantic discovery/mutation without reproducing the historical SIGSEGV before removing this blocker. |

Closed development blocker **R01**: Rust 1.88 is the declared workspace MSRV and the hosted MSRV job executes the required fmt/check/build/Clippy/tests/doctests/docs/release/fake/federation gate set. Rust 1.98.1 remains the development pin rather than being mislabeled as the minimum.

Closed development blocker **R09**: the real Rust Chromium matrix now exercises bounded per-file/count/total download quotas, CDP cancellation, crash/dead-instance relaunch, screenshot/download artifact lifecycle, stale refs and real multi-frame navigation. These tests retain disposable profiles and do not expand browser authority.

Supply-chain closure under **R15** now extends the existing native packaging evidence: `Supply-chain certification` evaluates the pinned Nix derivation, generates normalized reproducible CycloneDX SBOMs, builds x86_64/aarch64 certification bundles and emits GitHub artifact/SBOM attestations. `Packaging certification` separately retains normalized tar/deb reproducibility plus private install/execute/uninstall and tamper-safe removal evidence.

Live-matrix progress under **R06**: GNOME Shell 46.0 on a real Wayland login session executes the production AT-SPI backend against a disposable Zenity/GTK fixture, and a separate owner-approved GNOME portal run negotiates keyboard+pointer `ConnectToEIS` and explicit session shutdown. Hosted `Plasma Wayland live` executes the KWin 6 mailbox bridge through discovery/focus/resize/move/close/stale-ref lifecycle; the Sway fixture executes the native IPC path on a real headless compositor; and `Native X11 EWMH live` executes the X11 backend against Openbox. Hyprland remains unclosed because the hosted container lacks the dmabuf-capable parent/DRM path required by Aquamarine 0.15; focused portal input, coordinate/scaling behavior and broader multi-monitor/real-login coverage remain outside this closure.

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
