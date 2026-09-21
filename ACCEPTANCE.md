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
| A008 | User-level broker daemon. | FAIL | Source exists in crates/core, types, registry, policy, protocol and daemon; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |
| A009 | Versioned local IPC. | FAIL | Source exists in crates/core, types, registry, policy, protocol and daemon; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |
| A010 | Typed command registry. | FAIL | Source exists in crates/core, types, registry, policy, protocol and daemon; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |
| A011 | Capability model. | FAIL | Source exists in crates/core, types, registry, policy, protocol and daemon; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |
| A012 | Policy engine. | FAIL | Source exists in crates/core, types, registry, policy, protocol and daemon; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |
| A013 | Structured errors. | FAIL | Source exists in crates/core, types, registry, policy, protocol and daemon; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |
| A014 | Reference/stale-reference system. | FAIL | Source exists in crates/core, types, registry, policy, protocol and daemon; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |
| A015 | Audit/redaction. | FAIL | Source exists in crates/core, types, registry, policy, protocol and daemon; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |
| A016 | Backend capability discovery. | FAIL | Source exists in crates/core, types, registry, policy, protocol and daemon; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |
| A017 | Cancellation/timeouts. | FAIL | Source exists in crates/core, types, registry, policy, protocol and daemon; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |
| A018 | Dry-run for appropriate commands. | FAIL | Source exists in crates/core, types, registry, policy, protocol and daemon; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |

## Desktop

| ID | Original requirement | Status | Evidence / limitation |
|---|---|---|---|
| A019 | Environment/desktop/session detector. | FAIL | Source exists in crates/backends + bridges; docs/compatibility.md; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |
| A020 | AT-SPI app enumeration. | FAIL | Source exists in crates/backends + bridges; docs/compatibility.md; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |
| A021 | Normalized accessibility tree. | FAIL | Source exists in crates/backends + bridges; docs/compatibility.md; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |
| A022 | Semantic selector engine. | FAIL | Source exists in crates/backends + bridges; docs/compatibility.md; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |
| A023 | UI action invocation. | FAIL | Source exists in crates/backends + bridges; docs/compatibility.md; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |
| A024 | Window listing. | FAIL | Source exists in crates/backends + bridges; docs/compatibility.md; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |
| A025 | At least one real Wayland window/control route. | FAIL | Native compositor/portal routes compile, but no accepted live Wayland exercise exists. |
| A026 | Portal RemoteDesktop integration or complete implemented path with contract tests if live portal unavailable. | FAIL | Portal code and Rust tests execute, but EIS, restore-token and stream gaps remain. |
| A027 | ScreenCast/screenshot integration or explicit capability state. | PASS | Compiled source explicitly reports unavailable PipeWire pixels and exposes the implemented screenshot portal path honestly. |
| A028 | X11 fallback. | FAIL | Source exists in crates/backends + bridges; docs/compatibility.md; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |
| A029 | GNOME backend/bridge. | FAIL | Source exists in crates/backends + bridges; docs/compatibility.md; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |
| A030 | KDE backend/bridge. | FAIL | Source exists in crates/backends + bridges; docs/compatibility.md; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |
| A031 | Sway and/or Hyprland support path. | FAIL | Source exists in crates/backends + bridges; docs/compatibility.md; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |
| A032 | Input fallback is explicit and policy-gated. | FAIL | Source exists in crates/backends + bridges; docs/compatibility.md; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |

## Commands

| ID | Original requirement | Status | Evidence / limitation |
|---|---|---|---|
| A033 | `doctor`. | FAIL | Source exists in schemas/commands.json + backend source; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |
| A034 | capabilities discovery. | FAIL | Source exists in schemas/commands.json + backend source; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |
| A035 | app commands. | FAIL | Source exists in schemas/commands.json + backend source; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |
| A036 | window commands. | FAIL | Source exists in schemas/commands.json + backend source; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |
| A037 | UI inspect/find/invoke/text/value operations. | FAIL | Source exists in schemas/commands.json + backend source; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |
| A038 | pointer/keyboard operations. | FAIL | Source exists in schemas/commands.json + backend source; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |
| A039 | screen capture. | FAIL | Source exists in schemas/commands.json + backend source; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |
| A040 | clipboard separation read/write. | FAIL | Source exists in schemas/commands.json + backend source; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |
| A041 | process metadata/control at safe scope. | FAIL | Source exists in schemas/commands.json + backend source; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |
| A042 | scoped filesystem operations if included. | FAIL | Source exists in schemas/commands.json + backend source; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |
| A043 | restricted shell disabled by default if included. | NOT_APPLICABLE | No shell execution command is implemented; shell.exec grant rejected. |

## Front ends

| ID | Original requirement | Status | Evidence / limitation |
|---|---|---|---|
| A044 | CLI. | FAIL | Source exists in crates/cli, mcp, tui; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |
| A045 | JSON CLI mode. | FAIL | Source exists in crates/cli, mcp, tui; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |
| A046 | MCP using official Rust SDK. | FAIL | Source exists in crates/cli, mcp, tui; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |
| A047 | TUI/inspector or equivalent high-quality debugging surface. | FAIL | Source exists in crates/cli, mcp, tui; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |
| A048 | MCP does not bypass policy. | FAIL | Source exists in crates/cli, mcp, tui; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |
| A049 | MCP tool discovery/context-control strategy. | PASS | Eight discovery/gateway tools are documented and exercised by server/client integration tests. |

## Extensibility

| ID | Original requirement | Status | Evidence / limitation |
|---|---|---|---|
| A050 | Plugin manifest v1. | FAIL | Source exists in crates/plugin-sdk, plugin-host, recipes + adapters/example-plugin; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |
| A051 | Plugin protocol handshake. | FAIL | Protocol/name handshake authored; independent full version/schema attestation incomplete and untested. |
| A052 | Sandboxed plugin host. | FAIL | Bubblewrap/Landlock source compiles; hostile negative sandbox execution remains unverified. R10. |
| A053 | Example plugin. | FAIL | Source exists in crates/plugin-sdk, plugin-host, recipes + adapters/example-plugin; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |
| A054 | Recipe schema v1. | FAIL | Source exists in crates/plugin-sdk, plugin-host, recipes + adapters/example-plugin; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |
| A055 | Recipe validation. | FAIL | Source exists in crates/plugin-sdk, plugin-host, recipes + adapters/example-plugin; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |
| A056 | Recipe runner. | FAIL | Source exists in crates/plugin-sdk, plugin-host, recipes + adapters/example-plugin; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |
| A057 | Recipe fake-backend tests. | FAIL | Source exists in crates/plugin-sdk, plugin-host, recipes + adapters/example-plugin; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |
| A058 | Scaffold commands/templates. | FAIL | Source exists in crates/plugin-sdk, plugin-host, recipes + adapters/example-plugin; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |

## First-party adapters

| ID | Original requirement | Status | Evidence / limitation |
|---|---|---|---|
| A059 | Blender adapter implemented to a useful depth. | FAIL | Source exists in crates/adapters + adapters/blender; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |
| A060 | Browser/Chromium adapter implemented to a useful depth. | PASS | Real Rust CDP job executes launch, navigation, DOM/input, screenshot artifact, download/origin denial, stale refs and owned-profile cleanup. |
| A061 | Adapters have their own doctor/status. | FAIL | Source exists in crates/adapters + adapters/blender; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |
| A062 | No arbitrary application scripting exposed by default without high-risk capability. | FAIL | Source exists in crates/adapters + adapters/blender; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |

## Security

| ID | Original requirement | Status | Evidence / limitation |
|---|---|---|---|
| A063 | No root requirement for core. | FAIL | Source exists in crates/policy, protocol, core, daemon, plugin-host; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |
| A064 | Unix socket permissions and peer UID checks. | FAIL | Source exists in crates/policy, protocol, core, daemon, plugin-host; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |
| A065 | Filesystem scoping robust against traversal. | FAIL | Source exists in crates/policy, protocol, core, daemon, plugin-host; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |
| A066 | Secrets redacted from logs. | FAIL | Source exists in crates/policy, protocol, core, daemon, plugin-host; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |
| A067 | Clipboard and screenshots treated as sensitive. | FAIL | Source exists in crates/policy, protocol, core, daemon, plugin-host; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |
| A068 | Plugin environment scrubbed. | FAIL | Source exists in crates/policy, protocol, core, daemon, plugin-host; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |
| A069 | Landlock integration where available. | FAIL | Source exists in crates/policy, protocol, core, daemon, plugin-host; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |
| A070 | Bubblewrap integration or documented optional hardening. | FAIL | Source exists in crates/policy, protocol, core, daemon, plugin-host; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |
| A071 | No implicit `sudo`. | PASS | No sudo/elevation execution path; core requires a normal login user. |
| A072 | Confirmation mechanism cannot be self-approved by the LLM. | FAIL | Source exists in crates/policy, protocol, core, daemon, plugin-host; compiled and tested in the hosted baseline; the full criterion still lacks required live/conformance evidence. |
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
| A083 | Adapter tests. | FAIL | Rust adapter tests and real Chromium pass; real Blender remains unexecuted. |
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
| A092 | relevant JS/Python/shell linters run. | FAIL | Source syntax/contracts and workflow shell regressions pass; a distinct hosted Ruff/shellcheck/actionlint gate is not yet present. |
| A093 | release build run. | PASS | Locked workspace release build passes on x86_64 and ARM64. |

## Packaging

| ID | Original requirement | Status | Evidence / limitation |
|---|---|---|---|
| A094 | systemd user unit. | PASS | packaging/systemd-user/semwright.service included; no service enabled. |
| A095 | install/uninstall path. | PASS | packaging/install Python tools plus user documentation; real ELF install/uninstall not tested. |
| A096 | release workflow. | PASS | Fail-closed release definition and admission check; no publication/signing performed. |
| A097 | x86_64 artifact definition. | PASS | Native x86_64 runner builds the release workspace; packaged artifact installation remains unverified. |
| A098 | aarch64 artifact definition. | PASS | Native ARM runner/package definition; no build run. |
| A099 | checksums. | PASS | Source manifest and external ZIP SHA-256; not a binary release signature. |
| A100 | at least one distro packaging path plus tarball. | PASS | Tarball/.deb packager source; guarded, unexecuted with binaries. |
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

PASS: **60**, FAIL: **62**, LIVE_VERIFICATION_PENDING: **0**, NOT_APPLICABLE: **1**.

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

Provider Runtime expansion subtotal: **12 PASS, 0 FAIL**. MCP federation, public App Driver SDK/conformance, events/jobs breadth and application-specific deep drivers remain tracked separately in RELEASE_BLOCKERS.md.
