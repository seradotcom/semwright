# Acceptance resolution — all 123 original entries

**Accepted development baseline: the exact Git commit containing this document.**

**Overall: NOT V1.0 ACCEPTED.** The immutable original checklist is in
[docs/requirements/ACCEPTANCE_CHECKLIST.md](docs/requirements/ACCEPTANCE_CHECKLIST.md).

PASS below applies only to the exact requirement in its row. FAIL means the full criterion
is unmet; green baseline CI is not substituted for missing live desktop, application,
sandbox, federation, packaging, or security evidence. All execution claims refer only to
the exact baseline above. See [VERIFY.md](VERIFY.md) and
[RELEASE_BLOCKERS.md](RELEASE_BLOCKERS.md).

No claim that all requested functionality is implemented is made. Historical evidence from
other commits and local archives is not part of this acceptance decision.


## Repository

| ID | Original requirement | Status | Evidence / limitation |
|---|---|---|---|
| A001 | Real Rust workspace, not pseudocode. | PASS | Cargo workspace compiled and tested on hosted x86_64 and ARM64. |
| A002 | Builds from a clean checkout in the available environment. | PASS | Actions quality matrix builds debug/all-target and release on the exact baseline. |
| A003 | `Cargo.lock` committed. | PASS | Genuine locked graph is committed and enforced with `--locked`. |
| A004 | Public license files. | PASS | LICENSE-MIT and LICENSE-APACHE; full texts included. |
| A005 | README, contributing, security, changelog, code of conduct. | PASS | All requested governance/document files included. |
| A006 | No credentials/secrets. | PASS | No real credentials intentionally included; scoped static artifact scan, not an exhaustive secret audit. |
| A007 | No `target/` or bulky build outputs in final ZIP. | PASS | Source packager excludes build targets, caches and native executables. |

## Core architecture

| ID | Original requirement | Status | Evidence / limitation |
|---|---|---|---|
| A008 | User-level broker daemon. | PASS | Hosted release fake-smoke starts the user-level daemon and drives broker execution through the public CLI. |
| A009 | Versioned local IPC. | PASS | Versioned framed IPC is exercised by daemon/CLI E2E, strict protocol tests and bounded protocol fuzzing. |
| A010 | Typed command registry. | PASS | The typed registry and generated schemas compile; static and dynamic registration are covered by catalog transaction tests. |
| A011 | Capability model. | PASS | Capability descriptors, provenance, search, availability and Provider Runtime catalog behavior are integration-tested. |
| A012 | Policy engine. | PASS | Broker contract tests exercise allow/deny, read-only profiles, mutation denial and confirmation requirements; real drivers traverse the same policy path. |
| A013 | Structured errors. | PASS | Structured error codes/results round-trip through protocol, provider, driver and broker tests. |
| A014 | Reference/stale-reference system. | PASS | Session-bound refs and stale invalidation are tested in core plus real Chromium, X11 and live GTK/Qt AT-SPI paths. |
| A015 | Audit/redaction. | PASS | Audit/redaction tests and the daemon→CLI→recipe→audit fake-smoke execute in hosted CI. |
| A016 | Backend capability discovery. | PASS | Capability discovery is operation-specific and exercised by provider catalog, doctor and real driver integrations. |
| A017 | Cancellation/timeouts. | PASS | Provider Runtime, jobs and federation tests exercise bounded timeouts and cancellation propagation. |
| A018 | Dry-run for appropriate commands. | PASS | Broker dry-run regressions prove validation/policy planning occurs without invoking side effects. |

## Desktop

| ID | Original requirement | Status | Evidence / limitation |
|---|---|---|---|
| A019 | Environment/desktop/session detector. | PASS | Platform hosts expose session/desktop detection through doctor; Linux regression and native macOS matrices execute the host boundary. |
| A020 | AT-SPI app enumeration. | PASS | Dedicated hosted GTK and Qt AT-SPI jobs enumerate real disposable applications through the accessibility bus. |
| A021 | Normalized accessibility tree. | PASS | The hosted GTK/Qt fixtures produce normalized semantic snapshots and verify delta/full-resync behavior. |
| A022 | Semantic selector engine. | PASS | Selector unit/property/fuzz coverage executes against the normalized model; broker/fixture integration uses the same selector contract. |
| A023 | UI action invocation. | PASS | AT-SPI action/text implementations are broker-gated; live GTK/Qt fixtures execute real text mutation and core integration covers semantic invocation. |
| A024 | Window listing. | PASS | Real isolated X11/Xvfb integration exercises window discovery, refs and lifecycle epochs; AT-SPI fixtures also expose application windows. |
| A025 | At least one real Wayland window/control route. | PASS | GNOME Shell 46.0 Wayland live evidence executes a disposable GTK fixture through the production AT-SPI backend: discovery, snapshot, semantic text mutation, delta refresh, close/resync and stale-ref rejection. See `verification/live-gnome/gnome-wayland-atspi.json`. |
| A026 | Portal RemoteDesktop integration or complete implemented path with contract tests if live portal unavailable. | PASS | The allowed non-live alternative is met: RemoteDesktop/EIS, ScreenCast, restore-token and clipboard paths are implemented with real protocol and private D-Bus contract tests. Live portal consent remains a release matrix item. |
| A027 | ScreenCast/screenshot integration or explicit capability state. | PASS | Screenshot plus ScreenCast are implemented; hosted PipeWire integration captures a real synthetic frame and validates bounded raw-pixel conversion/artifact handling. |
| A028 | X11 fallback. | PASS | The X11 backend executes against isolated Xvfb with bounded blocking I/O, cancellation and lifecycle-epoch tests. |
| A029 | GNOME backend/bridge. | PASS | Real GNOME Shell 46.0 Wayland session exercised the documented primary GTK/AT-SPI route end to end on commit `6bab0cc`; the optional GJS bridge and portal-consent matrix remain broader follow-on coverage. |
| A030 | KDE backend/bridge. | FAIL | Source exists in crates/backends + bridges; docs/compatibility.md; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |
| A031 | Sway and/or Hyprland support path. | PASS | Sway and Hyprland backend paths compile and have command/peer/availability tests; full live compositor coverage remains R06. |
| A032 | Input fallback is explicit and policy-gated. | PASS | Input fallback is explicit and policy-gated; the real EIS protocol integration transmits keyboard, text, relative pointer, button and scroll events. |

## Commands

| ID | Original requirement | Status | Evidence / limitation |
|---|---|---|---|
| A033 | `doctor`. | PASS | semwright doctor is registered, broker-tested and reports platform/provider capability state. |
| A034 | capabilities discovery. | PASS | Capability list/search/describe/execute are exercised by Provider Runtime, CLI, MCP and driver smoke tests. |
| A035 | app commands. | PASS | Application discovery commands execute through AT-SPI fixtures and the broker command surface. |
| A036 | window commands. | PASS | Window commands are registered and exercised by the real X11 lifecycle integration. |
| A037 | UI inspect/find/invoke/text/value operations. | PASS | UI inspect/find/invoke/text/value commands are typed and broker-tested; live AT-SPI fixtures exercise snapshot and real text mutation. |
| A038 | pointer/keyboard operations. | PASS | Pointer/keyboard commands are policy-gated and the EIS protocol integration executes representative input events. |
| A039 | screen capture. | PASS | Screen capture paths execute through Chromium artifacts and Linux PipeWire/portal capture tests. |
| A040 | clipboard separation read/write. | PASS | Clipboard read/write are separate sensitive capabilities; portal clipboard grant lifecycle is covered by private D-Bus integration tests. |
| A041 | process metadata/control at safe scope. | PASS | Process metadata/control is limited to current-UID safe scope and executes in the hosted workspace tests. |
| A042 | scoped filesystem operations if included. | PASS | Scoped filesystem operations execute with FD-relative/openat2 semantics and native traversal/symlink regression coverage. |
| A043 | restricted shell disabled by default if included. | NOT_APPLICABLE | No shell execution command is implemented; shell.exec grant rejected. |

## Front ends

| ID | Original requirement | Status | Evidence / limitation |
|---|---|---|---|
| A044 | CLI. | PASS | The canonical semwright CLI is built and used by multiple daemon/broker/driver E2E smoke paths. |
| A045 | JSON CLI mode. | PASS | Machine-readable JSON mode is used and parsed by hosted smoke/conformance workflows. |
| A046 | MCP using official Rust SDK. | PASS | MCP uses the official Rust SDK; real SDK client E2E covers initialization, discovery, execution and read-only policy. |
| A047 | TUI/inspector or equivalent high-quality debugging surface. | PASS | `semwright-inspect` is a read-only nine-pane broker-backed TUI with filtering, refresh, audit/policy/UI plus session-scoped Jobs/Refs views and terminal-injection escaping tests. |
| A048 | MCP does not bypass policy. | PASS | Official-client MCP integration and broker tests prove MCP requests traverse normal policy and cannot self-approve. |
| A049 | MCP tool discovery/context-control strategy. | PASS | The bounded discovery/gateway surface is documented and exercised by official-SDK server/client integration tests; application commands remain behind search/describe/execute rather than static tool expansion. |

## Extensibility

| ID | Original requirement | Status | Evidence / limitation |
|---|---|---|---|
| A050 | Plugin manifest v1. | PASS | The original manifest requirement is satisfied; the pre-release wire protocol has since advanced fail-closed to Plugin Protocol v2 while retaining strict manifest hash/field validation. |
| A051 | Plugin protocol handshake. | PASS | Plugin Protocol v2 mutually attests plugin name, version and the SHA-256 digest of the complete ordered command descriptors; hosted mismatch tests reject version/descriptor drift before execution. |
| A052 | Sandboxed plugin host. | PASS | Hosted hostile-plugin tests execute through Bubblewrap + Landlock and prove mount boundaries, host-file/PID/loopback isolation, environment scrubbing and watchdog descendant cleanup. |
| A053 | Example plugin. | PASS | The example plugin is part of the compiled workspace and shares the typed plugin SDK contract. |
| A054 | Recipe schema v1. | PASS | Recipe schema v1 is committed, generated/validated and exercised by workspace/source checks. |
| A055 | Recipe validation. | PASS | Recipe validation executes in Rust tests and bounded recipe fuzzing. |
| A056 | Recipe runner. | PASS | Recipe runner executes in broker tests and the release fake-smoke path. |
| A057 | Recipe fake-backend tests. | PASS | Fake-backend recipe integration executes in hosted broker/quality tests. |
| A058 | Scaffold commands/templates. | PASS | Scaffold tooling/templates are present; hosted driver-conformance compiles a newly scaffolded driver. |

## First-party adapters

| ID | Original requirement | Status | Evidence / limitation |
|---|---|---|---|
| A059 | Blender adapter implemented to a useful depth. | PASS | Sandboxed Blender DriverProvider and the interactive add-on both execute against real Blender 4.5.14, including RNA/operator introspection, mutation, render and save. |
| A060 | Browser/Chromium adapter implemented to a useful depth. | PASS | Real Rust CDP job executes launch, navigation, DOM/input, screenshot artifact, download/origin denial, stale refs and owned-profile cleanup. |
| A061 | Adapters have their own doctor/status. | PASS | Browser, Blender, LibreOffice and other deep adapters expose explicit status/health/doctor-style capabilities exercised in native jobs. |
| A062 | No arbitrary application scripting exposed by default without high-risk capability. | PASS | Deep drivers expose curated typed operations; Blender explicitly rejects arbitrary Python/generic operator invoke and no raw browser/script gateway is enabled by default. |

## Security

| ID | Original requirement | Status | Evidence / limitation |
|---|---|---|---|
| A063 | No root requirement for core. | PASS | Core daemon runs as a normal login user and explicitly rejects unintended root operation outside test/fake paths. |
| A064 | Unix socket permissions and peer UID checks. | PASS | Unix socket/ticket permissions and same-UID peer validation are implemented in platform services/protocol and executed in workspace tests. |
| A065 | Filesystem scoping robust against traversal. | PASS | Filesystem scopes use FD-relative/openat2-style confinement with native traversal, symlink and package-extraction negative tests. |
| A066 | Secrets redacted from logs. | PASS | Audit/result redaction tests execute; sensitive payloads are excluded from audit and bounded verification logs. |
| A067 | Clipboard and screenshots treated as sensitive. | PASS | Clipboard and screenshot/ScreenCast commands are classified as sensitive and pass only through explicit policy/session paths. |
| A068 | Plugin environment scrubbed. | PASS | Sandbox launch clears/scrubs the plugin/driver environment; persistent DriverProvider conformance executes through the same helper. |
| A069 | Landlock integration where available. | PASS | Landlock is part of the executed driver sandbox path used by conformance and real LibreOffice/Blender drivers on Linux. |
| A070 | Bubblewrap integration or documented optional hardening. | PASS | Bubblewrap is executed in hosted driver conformance and real application-driver jobs; optional hardening/fallback boundaries are documented. |
| A071 | No implicit `sudo`. | PASS | No sudo/elevation execution path; core requires a normal login user. |
| A072 | Confirmation mechanism cannot be self-approved by the LLM. | PASS | Policy, broker and hostile-provider tests explicitly prove requesting models/metadata cannot self-approve confirmation. |
| A073 | Threat model documented. | PASS | SECURITY.md; docs/security.md; residual same-UID/app-process risks explicit. |

## Tests

| ID | Original requirement | Status | Evidence / limitation |
|---|---|---|---|
| A074 | Unit tests. | PASS | Rust workspace/all-target tests plus Python and Node suites pass on the exact baseline. |
| A075 | Property tests. | PASS | Rust property tests execute in the hosted workspace suite. |
| A076 | Golden tests. | PASS | Golden protocol comparisons execute in the hosted workspace suite. |
| A077 | Fake desktop integration tests. | PASS | Broker fake-backend integration and release fake-smoke execute in the quality matrix. |
| A078 | Protocol fuzz targets. | PASS | Bounded protocol fuzz target passes with failure-propagating `pipefail`. |
| A079 | Selector fuzz target. | PASS | Bounded selector fuzz target passes with failure-propagating `pipefail`. |
| A080 | Recipe fuzz target. | PASS | Bounded recipe fuzz target passes with failure-propagating `pipefail`. |
| A081 | Headless tests where feasible. | PASS | Hosted Rust/Python/Node/C, fake E2E and real isolated Chromium execute headlessly. |
| A082 | At least one full fake end-to-end workflow. | PASS | Release binaries run daemon → CLI → recipe → audit fake-smoke on both architectures. |
| A083 | Adapter tests. | PASS | Adapter tests now include real Chromium, real Blender, real LibreOffice/UNO, KiCad/MLT runtime evidence and isolated real OBS in addition to mocks/fixtures. |
| A084 | CI workflows. | PASS | Quality, security and native workflows are green on the exact baseline. |

## Quality gates

| ID | Original requirement | Status | Evidence / limitation |
|---|---|---|---|
| A085 | fmt passes. | PASS | `cargo fmt --all -- --check` passes on x86_64 and ARM64. |
| A086 | check passes. | PASS | Locked workspace/all-target/all-feature check passes on both architectures. |
| A087 | clippy with warnings denied passes. | PASS | Workspace/all-target/all-feature Clippy passes with `-D warnings`. |
| A088 | tests pass. | PASS | Rust, Python and Node suites pass on the exact baseline. |
| A089 | docs build. | PASS | Rustdoc builds with warnings denied; doctest command passes. |
| A090 | audit run. | PASS | `cargo audit --deny warnings` passes. |
| A091 | deny/license check run. | PASS | `cargo deny --locked check` passes. |
| A092 | relevant JS/Python/shell linters run. | PASS | Hosted `static-lints` runs pinned Ruff 0.13.2, ShellCheck and actionlint (including embedded workflow shell) and is green on the certified development line. |
| A093 | release build run. | PASS | Locked workspace release build passes on x86_64 and ARM64. |

## Packaging

| ID | Original requirement | Status | Evidence / limitation |
|---|---|---|---|
| A094 | systemd user unit. | PASS | packaging/systemd-user/semwright.service included; no service enabled. |
| A095 | install/uninstall path. | PASS | Hosted native x86_64/ARM64 packaging certification executes private user install, runs the installed binaries, uninstalls them, and verifies tamper-safe refusal. |
| A096 | release workflow. | PASS | Fail-closed release definition and admission check; no publication/signing performed. |
| A097 | x86_64 artifact definition. | PASS | Native x86_64 packaging job builds the five release executables, produces reproducible tar/deb artifacts and executes the user installation lifecycle. |
| A098 | aarch64 artifact definition. | PASS | Native ARM64 packaging job builds the five release executables, produces reproducible tar/deb artifacts and executes the user installation lifecycle. |
| A099 | checksums. | PASS | Source manifest and external ZIP SHA-256; not a binary release signature. |
| A100 | at least one distro packaging path plus tarball. | PASS | Hosted certification builds normalized tarballs and `.deb` packages twice, validates payloads and proves deterministic hashes on native x86_64/ARM64 runners. |
| A101 | Nix packaging/flake if feasible. | PASS | Nix package expression provided, no flake/lock/evaluation claimed. |

## Documentation

| ID | Original requirement | Status | Evidence / limitation |
|---|---|---|---|
| A102 | Architecture. | PASS | docs/architecture.md |
| A103 | Command reference. | PASS | docs/commands.md + schemas/commands.json |
| A104 | MCP setup. | PASS | docs/mcp.md |
| A105 | Plugin guide. | PASS | docs/plugins.md |
| A106 | Recipe guide. | PASS | docs/recipes.md |
| A107 | Security/permissions. | PASS | SECURITY.md + docs/permissions.md |
| A108 | Wayland explanation. | PASS | docs/wayland.md |
| A109 | Compatibility matrix. | PASS | docs/compatibility.md |
| A110 | Troubleshooting. | PASS | docs/troubleshooting.md |
| A111 | Manual testing matrix. | PASS | docs/manual-testing.md |
| A112 | `VERIFY.md`. | PASS | VERIFY.md |

## Honesty gate

| ID | Original requirement | Status | Evidence / limitation |
|---|---|---|---|
| A113 | Every live-desktop feature not actually exercised is labeled as unverified. | PASS | README/VERIFY/compatibility explicitly distinguish source from live support. |
| A114 | No fabricated benchmark. | PASS | Benchmark source only; no invented measurements. |
| A115 | No fabricated screenshot/GIF. | PASS | No demo screenshot/GIF is included; browser PNG was checked in memory. |
| A116 | No claim of universal Wayland support. | PASS | Wayland gaps and per-backend validation are explicit. |
| A117 | No “production-ready” claim if critical compile/tests fail. | PASS | Version is development source; full acceptance/release is blocked. |

## Final handoff

| ID | Original requirement | Status | Evidence / limitation |
|---|---|---|---|
| A118 | ZIP created. | PASS | Source archive generated; external archive-check records final SHA/path/CRC verification. |
| A119 | ZIP opens and contains top-level repo. | PASS | Archive checked for CRC errors and exactly one semwright/ top-level directory. |
| A120 | SHA-256 of ZIP reported. | PASS | External .sha256/check metadata accompanies the archive. |
| A121 | Final response links ZIP. | PASS | Final handoff includes a verified sandbox link to the actual archive. |
| A122 | Final response summarizes what was actually run. | PASS | VERIFY.md distinguishes exact-commit hosted Rust, source, fake-E2E and Chromium evidence from remaining live gaps. |
| A123 | Remaining live Linux test steps are listed, not hidden. | PASS | docs/manual-testing.md plus RELEASE_BLOCKERS.md. |

## Totals

PASS: **121**, FAIL: **1**, LIVE_VERIFICATION_PENDING: **0**, NOT_APPLICABLE: **1**.

These totals measure checklist resolution, not a percentage of software correctness.
A green compiler baseline does not replace missing live/security evidence, regardless of the
number of documents, tests authored, or individually passing component checks.

## Provider Runtime expansion acceptance

These criteria extend the original 123-entry checklist; they do not replace or renumber it.

| ID | Expansion requirement | Status | Evidence / limitation |
|---|---|---|---|
| P001 | Provider identity and namespace are explicit and cannot claim builtin authority. | PASS | ProviderIdentity validation plus negative namespace/authority tests. |
| P002 | Invocation provenance survives broker execution and audit. | PASS | Provider Runtime integration verifies provider/version/digest/generation in result and audit. |
| P003 | Dynamic provider registration/update/removal is atomic and revisioned. | PASS | Provider catalog transaction tests cover replace/remove and failed-batch rollback. |
| P004 | Stale catalog revisions fail rather than silently repaginating. | PASS | Catalog revision conflict tests. |
| P005 | Availability is operation-specific for dynamic providers. | PASS | One available fixture operation does not enable an unavailable sibling operation. |
| P006 | Terminal disconnect invalidates execution and cannot be reactivated by refresh. | PASS | Definitive-disconnect regression plus in-flight cancellation integration. |
| P007 | Provider capability changes/events are source-bound and do not require polling. | PASS | Broadcast refresh/event integration with broker-bound provenance. |
| P008 | Imported metadata cannot grant permission or self-approve confirmation. | PASS | Hostile-description policy/confirmation regression. |
| P009 | External schemas and values are resource-bounded consistently. | PASS | Depth/node/regex/ref budgets including Draft 7 dependency subschemas. |
| P010 | Result schemas, timeouts and cancellation are enforced at the broker boundary. | PASS | Provider Runtime result/timeout/cancel integration tests. |
| P011 | Duplicate/simultaneous provider ownership cannot overwrite an existing owner. | PASS | Atomic duplicate-registration regression. |
| P012 | Provider Runtime is exercised under the required hosted exact-commit gates. | PASS | Quality x86_64/ARM64, source contracts, dependency, coverage, fuzz and real Chromium checks on the commit containing this document. |

Provider Runtime expansion subtotal: **12 PASS, 0 FAIL**.

## MCP Federation expansion acceptance

| ID | Expansion requirement | Status | Evidence / limitation |
|---|---|---|---|
| F001 | External MCP identity/namespace is owner-assigned and cannot claim builtin authority. | PASS | `ExternalMcpProvider` uses ProviderIdentity plus namespace/authority rejection tests. |
| F002 | Upstream `tools/list` becomes bounded, namespaced, untrusted Semwright capabilities. | PASS | Real stdio fixture import and malformed/duplicate descriptor tests. |
| F003 | Federated execution always traverses broker policy/approval/audit. | PASS | Integration covers allowed execution and explicit policy denial. |
| F004 | Hostile descriptions/errors/results remain untrusted data and cannot grant authority. | PASS | Metadata/policy regressions plus bounded generic upstream errors. |
| F005 | Cancellation/timeouts propagate to an outstanding upstream request. | PASS | Federation cancellation integration test. |
| F006 | `tools/list_changed` refreshes the catalog transactionally without granting policy. | PASS | Dynamic refresh integration test. |
| F007 | Upstream crash/disconnect invalidates its provider generation fail-closed. | PASS | Crash invalidation integration test. |
| F008 | Owner upstream definitions are digest-pinned and managed independently of policy grants. | PASS | Atomic registry lifecycle/symlink/digest tests and local CLI management. |
| F009 | Federation executes under hosted exact-SHA workspace/coverage/fuzz gates. | PASS | Federation integration tests are part of the locked hosted workspace suite. |
| F010 | Spawned upstream MCP executables are sandboxed against the same Unix UID. | FAIL | Current stdio launcher is digest-pinned/environment-scrubbed but explicitly not a same-UID sandbox. |

MCP Federation expansion subtotal: **9 PASS, 1 FAIL**.

## App Driver SDK expansion acceptance

| ID | Expansion requirement | Status | Evidence / limitation |
|---|---|---|---|
| D001 | Driver manifest/protocol are versioned, strict and owner-assigned. | PASS | Driver SDK strict manifest/protocol tests. |
| D002 | Driver executable is owned/root, immutable to group/others and SHA-256 pinned. | PASS | Host verification plus wrong-digest/writable-executable negative tests. |
| D003 | Persistent handshake attests identity/version and capability catalog digest. | PASS | Real conformance fixture through DriverProvider. |
| D004 | Driver capabilities execute through the Provider Runtime and broker policy/audit. | PASS | Driver broker smoke and provider registration path. |
| D005 | Production driver host refuses unsandboxed launch and runs the fixture through bubblewrap + Landlock. | PASS | Hosted `driver-conformance` check plus local conformance. |
| D006 | Network/filesystem requests cannot exceed owner configuration. | PASS | Manifest/grant validation and network/interface fail-closed tests. |
| D007 | Driver conformance executes health, safe read-only capability and clean shutdown. | PASS | Hosted conformance fixture. |
| D008 | Scaffolded driver project compiles against the public SDK. | PASS | Conformance script creates and `cargo check`s a generated driver. |
| D009 | Hostile sandbox escape matrix covers filesystem/network/process/environment attacks. | PASS | Hosted hostile plugin and DriverProvider fixtures exercise filesystem mounts, host-file/PID/loopback isolation, environment scrubbing, RLIMIT enforcement and descendant cleanup through the production Linux sandbox launcher. |
| D010 | Driver protocol negotiates dynamic capabilities, events and cooperative cancellation. | FAIL | Protocol v1 intentionally rejects these interfaces until implemented/tested. |
| D011 | Driver registry/distribution supports safe search/install/update/removal. | PASS | Certified static/local `.swdp` distribution provides bounded package/index validation, compatibility resolution and non-executing install/update/remove with pinned digests. |
| D012 | A second non-browser/non-Blender application has a real deep driver integration. | PASS | Sandboxed LibreOffice/UNO executes real Writer/Calc/PDF operations through CLI -> broker -> DriverProvider in hosted CI. |

App Driver SDK expansion subtotal: **11 PASS, 1 FAIL**.

## LibreOffice deep-driver expansion acceptance

| ID | Expansion requirement | Status | Evidence / limitation |
|---|---|---|---|
| L001 | A real LibreOffice process executes through the persistent DriverProvider sandbox. | PASS | Hosted ignored integration test launches real UNO inside Bubblewrap + Landlock. |
| L002 | The full public path reaches LibreOffice without a core-specific execution bypass. | PASS | Hosted smoke covers CLI -> daemon -> broker policy/provenance -> DriverProvider -> UNO. |
| L003 | Writer create/read is round-tripped inside a scoped workspace. | PASS | ODT content is created, reopened and checked through UNO plus archive content validation. |
| L004 | Calc preserves typed numeric zero and supports bounded cell mutation. | PASS | ODS create/get/set smoke verifies `0` as numeric and a subsequent text mutation. |
| L005 | PDF export creates a new artifact and refuses silent overwrite. | PASS | Hosted/local smoke checks `%PDF-` output and a conflicting create fails. |
| L006 | LibreOffice runtime configuration is narrow and resource-bounded. | PASS | `/etc/libreoffice` and `/etc/fonts` are explicit read-only grants; process/FD/CPU/address-space/file-size limits are validated and enforced by the sandbox helper. |
| L007 | LibreOffice exposes the complete UNO surface or arbitrary macros/scripts. | FAIL | The driver intentionally exposes seven curated capabilities; full UNO introspection/coverage remains future scope. |

LibreOffice deep-driver expansion subtotal: **6 PASS, 1 FAIL**.

## Events and Jobs expansion acceptance

| ID | Expansion requirement | Status | Evidence / limitation |
|---|---|---|---|
| J001 | Events preserve broker-bound source/provider provenance and reject reserved-field smuggling. | PASS | Typed event-envelope tests cover provenance serialization, reserved fields and untrusted payload labelling. |
| J002 | Replay and live event delivery respect broker-session audience. | PASS | Broker/daemon integration filters private lifecycle events to the owning session. |
| J003 | Jobs are bounded and session-scoped rather than an unbounded global task store. | PASS | JobStore limits broker/session/active counts and retained result size; cross-session reads fail. |
| J004 | A nested job request re-enters normal schema, policy, confirmation, provenance and audit enforcement. | PASS | Read-only completion and observe-to-mutation denial integration tests. |
| J005 | Cancellation is idempotent and can reach a blocked dynamic provider without waiting behind its execution gate. | PASS | Core and Provider Runtime cancellation regressions. |
| J006 | Session revocation cancels and forgets only that session's active jobs. | PASS | Revocation regression covers ownership and cleanup. |
| J007 | Job lifecycle events are source-tagged and private to the owning session. | PASS | Queued/started/cancel-requested/terminal event integration plus audience filtering. |
| J008 | Providers expose a general, measured progress and artifact contract for long operations. | PASS | Provider Runtime correlates bounded `JobProgress`/`JobArtifact` signals to the owning session-scoped job; integration tests retain the measured state through `jobs.get`. |
| J009 | MCP Tasks and driver protocol job/event interfaces are negotiated and conformant. | PASS | MCP Tasks create/get/result/cancel run through the official SDK, while Driver Protocol v2 has sandboxed conformance for child events, progress/artifacts, dynamic capability invalidation and cooperative cancellation; v1 remains compatibility-only and fail-closed for these interfaces. |

Events and Jobs expansion subtotal: **9 PASS, 0 FAIL**.

## OBS deep-driver expansion acceptance

| ID | Expansion requirement | Status | Evidence / limitation |
|---|---|---|---|
| O001 | OBS integration exposes a curated, strict capability surface rather than arbitrary upstream requests. | PASS | 65 descriptor-pinned capabilities with strict schemas; no generic raw request gateway is registered. |
| O002 | Production Rust client negotiates/authenticates obs-websocket 5.x with bounded correlation and generations. | PASS | Protocol/unit matrix plus independent fake-server integration exercises auth, request IDs, reconnects, late/duplicate responses and malformed wire input. |
| O003 | Event ingestion is bounded and stale state is invalidated after loss/reconnect. | PASS | Event-flood integration and regression tests cover queue bounds, dropped-event accounting, generation invalidation and cache/ref staleness. |
| O004 | OBS driver executes through the real Semwright Driver Host and broker policy path. | PASS | Hosted conformance runs the release driver through Bubblewrap + Landlock and a full broker/policy/CLI smoke against the independent fake OBS server. |
| O005 | OBS-specific fuzz targets execute in hosted CI. | PASS | Six bounded targets cover messages, events, responses, refs, bounded JSON and capability mapping. |
| O006 | Production Rust transport executes against a real isolated OBS Studio instance. | PASS | Hosted `real-obs` runs OBS Studio 30.0.2 + obs-websocket 5.3.4 in private namespaces/Xvfb and passes authenticated read-only `GetVersion`/`GetSceneList`; recording and streaming remain off. |
| O007 | The OBS driver child emits broker-native events/jobs/progress through a negotiated Driver Protocol interface. | FAIL | The platform now provides and tests Driver Protocol v2, but the OBS manifest intentionally remains on v1 and therefore does not yet negotiate child events, cooperative cancellation, dynamic capabilities or generic progress/artifact transport. |
| O008 | OBS secrets/network authority are least-privilege production contracts. | FAIL | Loopback is the driver default and owner network opt-in is enforced, but generic secure secret references and port-scoped network grants remain future Driver SDK work. |

OBS deep-driver expansion subtotal: **6 PASS, 2 FAIL**.

The original 123-entry totals above are intentionally unchanged by these expansion tables.
Real Blender introspection, broader application coverage, richer long-operation contracts and the
remaining universal Linux/runtime blockers stay tracked in RELEASE_BLOCKERS.md.
