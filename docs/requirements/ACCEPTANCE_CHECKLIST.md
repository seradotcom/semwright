# Acceptance Checklist — v1.0-grade handoff

The coding chat should use this as a hard completion checklist.

## Repository

- [ ] Real Rust workspace, not pseudocode.
- [ ] Builds from a clean checkout in the available environment.
- [ ] `Cargo.lock` committed.
- [ ] Public license files.
- [ ] README, contributing, security, changelog, code of conduct.
- [ ] No credentials/secrets.
- [ ] No `target/` or bulky build outputs in final ZIP.

## Core architecture

- [ ] User-level broker daemon.
- [ ] Versioned local IPC.
- [ ] Typed command registry.
- [ ] Capability model.
- [ ] Policy engine.
- [ ] Structured errors.
- [ ] Reference/stale-reference system.
- [ ] Audit/redaction.
- [ ] Backend capability discovery.
- [ ] Cancellation/timeouts.
- [ ] Dry-run for appropriate commands.

## Desktop

- [ ] Environment/desktop/session detector.
- [ ] AT-SPI app enumeration.
- [ ] Normalized accessibility tree.
- [ ] Semantic selector engine.
- [ ] UI action invocation.
- [ ] Window listing.
- [ ] At least one real Wayland window/control route.
- [ ] Portal RemoteDesktop integration or complete implemented path with contract tests if live portal unavailable.
- [ ] ScreenCast/screenshot integration or explicit capability state.
- [ ] X11 fallback.
- [ ] GNOME backend/bridge.
- [ ] KDE backend/bridge.
- [ ] Sway and/or Hyprland support path.
- [ ] Input fallback is explicit and policy-gated.

## Commands

- [ ] `doctor`.
- [ ] capabilities discovery.
- [ ] app commands.
- [ ] window commands.
- [ ] UI inspect/find/invoke/text/value operations.
- [ ] pointer/keyboard operations.
- [ ] screen capture.
- [ ] clipboard separation read/write.
- [ ] process metadata/control at safe scope.
- [ ] scoped filesystem operations if included.
- [ ] restricted shell disabled by default if included.

## Front ends

- [ ] CLI.
- [ ] JSON CLI mode.
- [ ] MCP using official Rust SDK.
- [ ] TUI/inspector or equivalent high-quality debugging surface.
- [ ] MCP does not bypass policy.
- [ ] MCP tool discovery/context-control strategy.

## Extensibility

- [ ] Plugin manifest v1.
- [ ] Plugin protocol handshake.
- [ ] Sandboxed plugin host.
- [ ] Example plugin.
- [ ] Recipe schema v1.
- [ ] Recipe validation.
- [ ] Recipe runner.
- [ ] Recipe fake-backend tests.
- [ ] Scaffold commands/templates.

## First-party adapters

- [ ] Blender adapter implemented to a useful depth.
- [ ] Browser/Chromium adapter implemented to a useful depth.
- [ ] Adapters have their own doctor/status.
- [ ] No arbitrary application scripting exposed by default without high-risk capability.

## Security

- [ ] No root requirement for core.
- [ ] Unix socket permissions and peer UID checks.
- [ ] Filesystem scoping robust against traversal.
- [ ] Secrets redacted from logs.
- [ ] Clipboard and screenshots treated as sensitive.
- [ ] Plugin environment scrubbed.
- [ ] Landlock integration where available.
- [ ] Bubblewrap integration or documented optional hardening.
- [ ] No implicit `sudo`.
- [ ] Confirmation mechanism cannot be self-approved by the LLM.
- [ ] Threat model documented.

## Tests

- [ ] Unit tests.
- [ ] Property tests.
- [ ] Golden tests.
- [ ] Fake desktop integration tests.
- [ ] Protocol fuzz targets.
- [ ] Selector fuzz target.
- [ ] Recipe fuzz target.
- [ ] Headless tests where feasible.
- [ ] At least one full fake end-to-end workflow.
- [ ] Adapter tests.
- [ ] CI workflows.

## Quality gates

- [ ] fmt passes.
- [ ] check passes.
- [ ] clippy with warnings denied passes.
- [ ] tests pass.
- [ ] docs build.
- [ ] audit run.
- [ ] deny/license check run.
- [ ] relevant JS/Python/shell linters run.
- [ ] release build run.

## Packaging

- [ ] systemd user unit.
- [ ] install/uninstall path.
- [ ] release workflow.
- [ ] x86_64 artifact definition.
- [ ] aarch64 artifact definition.
- [ ] checksums.
- [ ] at least one distro packaging path plus tarball.
- [ ] Nix packaging/flake if feasible.

## Documentation

- [ ] Architecture.
- [ ] Command reference.
- [ ] MCP setup.
- [ ] Plugin guide.
- [ ] Recipe guide.
- [ ] Security/permissions.
- [ ] Wayland explanation.
- [ ] Compatibility matrix.
- [ ] Troubleshooting.
- [ ] Manual testing matrix.
- [ ] `VERIFY.md`.

## Honesty gate

- [ ] Every live-desktop feature not actually exercised is labeled as unverified.
- [ ] No fabricated benchmark.
- [ ] No fabricated screenshot/GIF.
- [ ] No claim of universal Wayland support.
- [ ] No “production-ready” claim if critical compile/tests fail.

## Final handoff

- [ ] ZIP created.
- [ ] ZIP opens and contains top-level repo.
- [ ] SHA-256 of ZIP reported.
- [ ] Final response links ZIP.
- [ ] Final response summarizes what was actually run.
- [ ] Remaining live Linux test steps are listed, not hidden.
