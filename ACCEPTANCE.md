# Acceptance resolution — all 123 original entries

**Overall: NOT V1.0 ACCEPTED.** The immutable original checklist is in
[docs/requirements/ACCEPTANCE_CHECKLIST.md](docs/requirements/ACCEPTANCE_CHECKLIST.md).

PASS below applies only to the exact artifact/presence or scoped evidence requirement in
its row. FAIL means the full criterion is unmet, including blocked/unexecuted gates; it
does not invent an observed Rust compiler failure. LIVE_VERIFICATION_PENDING is deliberately
not used to hide that Rust itself was never compiled. See [VERIFY.md](VERIFY.md) and
[RELEASE_BLOCKERS.md](RELEASE_BLOCKERS.md).

No claim that all requested functionality is implemented is made. Packaging/checksum
rows describe the final source archive, not a successful binary release. Their external
archive-check metadata is outside the ZIP to avoid self-referential archive hashes.


## Repository

| ID | Original requirement | Status | Evidence / limitation |
|---|---|---|---|
| A001 | Real Rust workspace, not pseudocode. | PASS | Cargo.toml; 15 Rust crates + example package; source present, not compiled. |
| A002 | Builds from a clean checkout in the available environment. | FAIL | Requirement unmet: no Rust toolchain/dependency cache or build. Not an observed compiler error. |
| A003 | `Cargo.lock` committed. | FAIL | No Cargo.lock fabricated; actual dependency resolution is required. |
| A004 | Public license files. | PASS | LICENSE-MIT and LICENSE-APACHE; full texts included. |
| A005 | README, contributing, security, changelog, code of conduct. | PASS | All requested governance/document files included. |
| A006 | No credentials/secrets. | PASS | No real credentials intentionally included; scoped static artifact scan, not an exhaustive secret audit. |
| A007 | No `target/` or bulky build outputs in final ZIP. | PASS | Source packager excludes build targets, caches and native executables. |

## Core architecture

| ID | Original requirement | Status | Evidence / limitation |
|---|---|---|---|
| A008 | User-level broker daemon. | FAIL | Source exists in crates/core, types, registry, policy, protocol and daemon; behavior/complete acceptance unverified without Rust build and required live tests. |
| A009 | Versioned local IPC. | FAIL | Source exists in crates/core, types, registry, policy, protocol and daemon; behavior/complete acceptance unverified without Rust build and required live tests. |
| A010 | Typed command registry. | FAIL | Source exists in crates/core, types, registry, policy, protocol and daemon; behavior/complete acceptance unverified without Rust build and required live tests. |
| A011 | Capability model. | FAIL | Source exists in crates/core, types, registry, policy, protocol and daemon; behavior/complete acceptance unverified without Rust build and required live tests. |
| A012 | Policy engine. | FAIL | Source exists in crates/core, types, registry, policy, protocol and daemon; behavior/complete acceptance unverified without Rust build and required live tests. |
| A013 | Structured errors. | FAIL | Source exists in crates/core, types, registry, policy, protocol and daemon; behavior/complete acceptance unverified without Rust build and required live tests. |
| A014 | Reference/stale-reference system. | FAIL | Source exists in crates/core, types, registry, policy, protocol and daemon; behavior/complete acceptance unverified without Rust build and required live tests. |
| A015 | Audit/redaction. | FAIL | Source exists in crates/core, types, registry, policy, protocol and daemon; behavior/complete acceptance unverified without Rust build and required live tests. |
| A016 | Backend capability discovery. | FAIL | Source exists in crates/core, types, registry, policy, protocol and daemon; behavior/complete acceptance unverified without Rust build and required live tests. |
| A017 | Cancellation/timeouts. | FAIL | Source exists in crates/core, types, registry, policy, protocol and daemon; behavior/complete acceptance unverified without Rust build and required live tests. |
| A018 | Dry-run for appropriate commands. | FAIL | Source exists in crates/core, types, registry, policy, protocol and daemon; behavior/complete acceptance unverified without Rust build and required live tests. |

## Desktop

| ID | Original requirement | Status | Evidence / limitation |
|---|---|---|---|
| A019 | Environment/desktop/session detector. | FAIL | Source exists in crates/backends + bridges; docs/compatibility.md; behavior/complete acceptance unverified without Rust build and required live tests. |
| A020 | AT-SPI app enumeration. | FAIL | Source exists in crates/backends + bridges; docs/compatibility.md; behavior/complete acceptance unverified without Rust build and required live tests. |
| A021 | Normalized accessibility tree. | FAIL | Source exists in crates/backends + bridges; docs/compatibility.md; behavior/complete acceptance unverified without Rust build and required live tests. |
| A022 | Semantic selector engine. | FAIL | Source exists in crates/backends + bridges; docs/compatibility.md; behavior/complete acceptance unverified without Rust build and required live tests. |
| A023 | UI action invocation. | FAIL | Source exists in crates/backends + bridges; docs/compatibility.md; behavior/complete acceptance unverified without Rust build and required live tests. |
| A024 | Window listing. | FAIL | Source exists in crates/backends + bridges; docs/compatibility.md; behavior/complete acceptance unverified without Rust build and required live tests. |
| A025 | At least one real Wayland window/control route. | FAIL | Native compositor/portal routes authored, but no Rust compilation or live Wayland exercise. |
| A026 | Portal RemoteDesktop integration or complete implemented path with contract tests if live portal unavailable. | FAIL | Native Notify path source present; Rust tests unrun and EIS/restore/stream gaps remain. |
| A027 | ScreenCast/screenshot integration or explicit capability state. | PASS | Source explicitly reports unavailable PipeWire pixels; Screenshot portal path authored, uncompiled. |
| A028 | X11 fallback. | FAIL | Source exists in crates/backends + bridges; docs/compatibility.md; behavior/complete acceptance unverified without Rust build and required live tests. |
| A029 | GNOME backend/bridge. | FAIL | Source exists in crates/backends + bridges; docs/compatibility.md; behavior/complete acceptance unverified without Rust build and required live tests. |
| A030 | KDE backend/bridge. | FAIL | Source exists in crates/backends + bridges; docs/compatibility.md; behavior/complete acceptance unverified without Rust build and required live tests. |
| A031 | Sway and/or Hyprland support path. | FAIL | Source exists in crates/backends + bridges; docs/compatibility.md; behavior/complete acceptance unverified without Rust build and required live tests. |
| A032 | Input fallback is explicit and policy-gated. | FAIL | Source exists in crates/backends + bridges; docs/compatibility.md; behavior/complete acceptance unverified without Rust build and required live tests. |

## Commands

| ID | Original requirement | Status | Evidence / limitation |
|---|---|---|---|
| A033 | `doctor`. | FAIL | Source exists in schemas/commands.json + backend source; behavior/complete acceptance unverified without Rust build and required live tests. |
| A034 | capabilities discovery. | FAIL | Source exists in schemas/commands.json + backend source; behavior/complete acceptance unverified without Rust build and required live tests. |
| A035 | app commands. | FAIL | Source exists in schemas/commands.json + backend source; behavior/complete acceptance unverified without Rust build and required live tests. |
| A036 | window commands. | FAIL | Source exists in schemas/commands.json + backend source; behavior/complete acceptance unverified without Rust build and required live tests. |
| A037 | UI inspect/find/invoke/text/value operations. | FAIL | Source exists in schemas/commands.json + backend source; behavior/complete acceptance unverified without Rust build and required live tests. |
| A038 | pointer/keyboard operations. | FAIL | Source exists in schemas/commands.json + backend source; behavior/complete acceptance unverified without Rust build and required live tests. |
| A039 | screen capture. | FAIL | Source exists in schemas/commands.json + backend source; behavior/complete acceptance unverified without Rust build and required live tests. |
| A040 | clipboard separation read/write. | FAIL | Source exists in schemas/commands.json + backend source; behavior/complete acceptance unverified without Rust build and required live tests. |
| A041 | process metadata/control at safe scope. | FAIL | Source exists in schemas/commands.json + backend source; behavior/complete acceptance unverified without Rust build and required live tests. |
| A042 | scoped filesystem operations if included. | FAIL | Source exists in schemas/commands.json + backend source; behavior/complete acceptance unverified without Rust build and required live tests. |
| A043 | restricted shell disabled by default if included. | NOT_APPLICABLE | No shell execution command is implemented; shell.exec grant rejected. |

## Front ends

| ID | Original requirement | Status | Evidence / limitation |
|---|---|---|---|
| A044 | CLI. | FAIL | Source exists in crates/cli, mcp, tui; behavior/complete acceptance unverified without Rust build and required live tests. |
| A045 | JSON CLI mode. | FAIL | Source exists in crates/cli, mcp, tui; behavior/complete acceptance unverified without Rust build and required live tests. |
| A046 | MCP using official Rust SDK. | FAIL | Source exists in crates/cli, mcp, tui; behavior/complete acceptance unverified without Rust build and required live tests. |
| A047 | TUI/inspector or equivalent high-quality debugging surface. | FAIL | Source exists in crates/cli, mcp, tui; behavior/complete acceptance unverified without Rust build and required live tests. |
| A048 | MCP does not bypass policy. | FAIL | Source exists in crates/cli, mcp, tui; behavior/complete acceptance unverified without Rust build and required live tests. |
| A049 | MCP tool discovery/context-control strategy. | PASS | Eight discovery/gateway tools documented and implemented in source; runtime remains blocked. |

## Extensibility

| ID | Original requirement | Status | Evidence / limitation |
|---|---|---|---|
| A050 | Plugin manifest v1. | FAIL | Source exists in crates/plugin-sdk, plugin-host, recipes + adapters/example-plugin; behavior/complete acceptance unverified without Rust build and required live tests. |
| A051 | Plugin protocol handshake. | FAIL | Protocol/name handshake authored; independent full version/schema attestation incomplete and untested. |
| A052 | Sandboxed plugin host. | FAIL | Bubblewrap/Landlock source present; not compiled/executed or negative-tested. R10. |
| A053 | Example plugin. | FAIL | Source exists in crates/plugin-sdk, plugin-host, recipes + adapters/example-plugin; behavior/complete acceptance unverified without Rust build and required live tests. |
| A054 | Recipe schema v1. | FAIL | Source exists in crates/plugin-sdk, plugin-host, recipes + adapters/example-plugin; behavior/complete acceptance unverified without Rust build and required live tests. |
| A055 | Recipe validation. | FAIL | Source exists in crates/plugin-sdk, plugin-host, recipes + adapters/example-plugin; behavior/complete acceptance unverified without Rust build and required live tests. |
| A056 | Recipe runner. | FAIL | Source exists in crates/plugin-sdk, plugin-host, recipes + adapters/example-plugin; behavior/complete acceptance unverified without Rust build and required live tests. |
| A057 | Recipe fake-backend tests. | FAIL | Source exists in crates/plugin-sdk, plugin-host, recipes + adapters/example-plugin; behavior/complete acceptance unverified without Rust build and required live tests. |
| A058 | Scaffold commands/templates. | FAIL | Source exists in crates/plugin-sdk, plugin-host, recipes + adapters/example-plugin; behavior/complete acceptance unverified without Rust build and required live tests. |

## First-party adapters

| ID | Original requirement | Status | Evidence / limitation |
|---|---|---|---|
| A059 | Blender adapter implemented to a useful depth. | FAIL | Source exists in crates/adapters + adapters/blender; behavior/complete acceptance unverified without Rust build and required live tests. |
| A060 | Browser/Chromium adapter implemented to a useful depth. | FAIL | Source exists in crates/adapters + adapters/blender; behavior/complete acceptance unverified without Rust build and required live tests. |
| A061 | Adapters have their own doctor/status. | FAIL | Source exists in crates/adapters + adapters/blender; behavior/complete acceptance unverified without Rust build and required live tests. |
| A062 | No arbitrary application scripting exposed by default without high-risk capability. | FAIL | Source exists in crates/adapters + adapters/blender; behavior/complete acceptance unverified without Rust build and required live tests. |

## Security

| ID | Original requirement | Status | Evidence / limitation |
|---|---|---|---|
| A063 | No root requirement for core. | FAIL | Source exists in crates/policy, protocol, core, daemon, plugin-host; behavior/complete acceptance unverified without Rust build and required live tests. |
| A064 | Unix socket permissions and peer UID checks. | FAIL | Source exists in crates/policy, protocol, core, daemon, plugin-host; behavior/complete acceptance unverified without Rust build and required live tests. |
| A065 | Filesystem scoping robust against traversal. | FAIL | Source exists in crates/policy, protocol, core, daemon, plugin-host; behavior/complete acceptance unverified without Rust build and required live tests. |
| A066 | Secrets redacted from logs. | FAIL | Source exists in crates/policy, protocol, core, daemon, plugin-host; behavior/complete acceptance unverified without Rust build and required live tests. |
| A067 | Clipboard and screenshots treated as sensitive. | FAIL | Source exists in crates/policy, protocol, core, daemon, plugin-host; behavior/complete acceptance unverified without Rust build and required live tests. |
| A068 | Plugin environment scrubbed. | FAIL | Source exists in crates/policy, protocol, core, daemon, plugin-host; behavior/complete acceptance unverified without Rust build and required live tests. |
| A069 | Landlock integration where available. | FAIL | Source exists in crates/policy, protocol, core, daemon, plugin-host; behavior/complete acceptance unverified without Rust build and required live tests. |
| A070 | Bubblewrap integration or documented optional hardening. | FAIL | Source exists in crates/policy, protocol, core, daemon, plugin-host; behavior/complete acceptance unverified without Rust build and required live tests. |
| A071 | No implicit `sudo`. | PASS | No sudo/elevation execution path; core requires a normal login user. |
| A072 | Confirmation mechanism cannot be self-approved by the LLM. | FAIL | Source exists in crates/policy, protocol, core, daemon, plugin-host; behavior/complete acceptance unverified without Rust build and required live tests. |
| A073 | Threat model documented. | PASS | SECURITY.md; docs/security.md; residual same-UID/app-process risks explicit. |

## Tests

| ID | Original requirement | Status | Evidence / limitation |
|---|---|---|---|
| A074 | Unit tests. | FAIL | 71 Python and 20 Node tests pass; all Rust unit tests unexecuted. Full requirement not met. |
| A075 | Property tests. | FAIL | Source exists in Rust/fixture/fuzz source and verification logs; behavior/complete acceptance unverified without Rust build and required live tests. |
| A076 | Golden tests. | FAIL | Golden hello vector + Rust comparison authored; Rust comparison not executed. |
| A077 | Fake desktop integration tests. | FAIL | Source exists in Rust/fixture/fuzz source and verification logs; behavior/complete acceptance unverified without Rust build and required live tests. |
| A078 | Protocol fuzz targets. | FAIL | Five fuzz targets and seeds authored; not compiled or fuzzed. No claim of passed smoke runs. |
| A079 | Selector fuzz target. | FAIL | Five fuzz targets and seeds authored; not compiled or fuzzed. No claim of passed smoke runs. |
| A080 | Recipe fuzz target. | FAIL | Five fuzz targets and seeds authored; not compiled or fuzzed. No claim of passed smoke runs. |
| A081 | Headless tests where feasible. | PASS | Python/Node/C and isolated real Chromium CDP executed; not Rust/desktop conformance. |
| A082 | At least one full fake end-to-end workflow. | FAIL | Full fake-export recipe, broker test and smoke script authored; Rust E2E was not run. |
| A083 | Adapter tests. | FAIL | Python mocked-Blender and live Python CDP tests passed; real Blender/Rust adapter paths untested. |
| A084 | CI workflows. | PASS | .github/workflows contains source/Rust/security/coverage/release definitions; none executed on GitHub. |

## Quality gates

| ID | Original requirement | Status | Evidence / limitation |
|---|---|---|---|
| A085 | fmt passes. | FAIL | Required gate unavailable/unexecuted; see VERIFY.md. Python/JS passes are not substituted. |
| A086 | check passes. | FAIL | Required gate unavailable/unexecuted; see VERIFY.md. Python/JS passes are not substituted. |
| A087 | clippy with warnings denied passes. | FAIL | Required gate unavailable/unexecuted; see VERIFY.md. Python/JS passes are not substituted. |
| A088 | tests pass. | FAIL | Required gate unavailable/unexecuted; see VERIFY.md. Python/JS passes are not substituted. |
| A089 | docs build. | FAIL | Required gate unavailable/unexecuted; see VERIFY.md. Python/JS passes are not substituted. |
| A090 | audit run. | FAIL | Required gate unavailable/unexecuted; see VERIFY.md. Python/JS passes are not substituted. |
| A091 | deny/license check run. | FAIL | Required gate unavailable/unexecuted; see VERIFY.md. Python/JS passes are not substituted. |
| A092 | relevant JS/Python/shell linters run. | FAIL | Syntax checks and language tests ran; Ruff/shellcheck/actionlint unavailable; no full linter pass. |
| A093 | release build run. | FAIL | Required gate unavailable/unexecuted; see VERIFY.md. Python/JS passes are not substituted. |

## Packaging

| ID | Original requirement | Status | Evidence / limitation |
|---|---|---|---|
| A094 | systemd user unit. | PASS | packaging/systemd-user/semwright.service included; no service enabled. |
| A095 | install/uninstall path. | PASS | packaging/install Python tools plus user documentation; real ELF install/uninstall not tested. |
| A096 | release workflow. | PASS | Fail-closed release definition and admission check; no publication/signing performed. |
| A097 | x86_64 artifact definition. | PASS | Native runner/package definition; no Rust binary built. |
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
| A122 | Final response summarizes what was actually run. | PASS | Final handoff distinguishes Python/JS/C/CDP checks from uncompiled Rust. |
| A123 | Remaining live Linux test steps are listed, not hidden. | PASS | docs/manual-testing.md plus RELEASE_BLOCKERS.md. |

## Totals

PASS: **41**, FAIL: **81**, LIVE_VERIFICATION_PENDING: **0**, NOT_APPLICABLE: **1**.

These totals measure checklist resolution, not a percentage of software correctness.
A missing compiler and missing live/security evidence block release regardless of the
number of documents, tests authored, or individually passing component checks.
