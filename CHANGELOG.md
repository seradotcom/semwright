# Changelog

## Unreleased

- Replaced temporary workstream-letter names in CI, source-backup tooling and cross-app evidence with component-oriented Composition, Audio, Project Graph, Godot, Blender and Effect Conformance terminology.
- Replaced temporary agent-role names in the AV composition public API and integration documentation with durable component-oriented names (`AvStageAdapter`, `AvArtifactRoutes`, and `av_stage_commands`).
- Separated public product/contributor documentation from temporary development orchestration; internal handoff/master-prompt material is no longer part of the current tree, while technical history and exact-SHA evidence remain preserved.
- Added a bundled three-step cross-platform quick start for native installs; portable installers now lead directly to the safe, idempotent `semwright setup` onboarding path.
- Added `semwright setup` for safe cross-platform first-run configuration and MCP snippet generation; installers now point directly to this onboarding path.
- Integrated portable platform contracts and native macOS/Windows hosts, preserving distinct
  hosted, interactive and physical-hardware verification levels.
- Added Provider Runtime, governed sandboxed MCP federation, persistent Driver Protocol v2,
  bounded events/jobs/artifacts, local driver distribution and application-specific drivers.
- Added Workflow Distillation and Agent Skills compatibility with broker policy re-entry,
  descriptor-drift checks and explicit verification/replay/promotion gates.
- Implemented AT-SPI delta recovery, portal/EIS lifecycle and bounded PipeWire capture;
  added native packaging reproducibility, Nix, SBOM and attestation workflows.
- Hardened security-review bundle generation: reject existing/unsafe output destinations,
  preserve previous bundles, create private output and checksum `BASELINE_SHA`.
- Escape terminal controls in CLI human diagnostics and JSON metadata without changing
  parsed values; remove the obsolete hardcoded PipeWire-unimplemented diagnostic.
- Reconcile current documentation with commit-scoped evidence. The pre-R16 observation
  retains a Windows ARM64 failure, unresolved parallel work and R16 OPEN; no release is declared.

- Renamed the canonical user CLI from `computerctl` to `semwright` across build targets,
  packaging, workflows, examples, smoke tests and documentation.
- Renamed the advertised MCP gateway tools from the legacy `computer_*` names to
  `semwright_*`; `capabilities_search` and `capabilities_describe` remain protocol-neutral.
- New installations publish only `semwright`; the uninstaller still recognizes historical
  `computerctl` manifests so pre-1.0 installs can be removed safely.
- Plugin Protocol v2 now mutually attests plugin name, version and the SHA-256 digest of
  the complete command descriptors before execution. Pre-v1-release Plugin Protocol v1
  manifests/children are rejected rather than silently accepted without schema attestation.
- Added a hosted hostile-plugin sandbox regression matrix covering filesystem grants,
  network/PID isolation, environment scrubbing and timeout descendant cleanup.


## 0.9.0-dev.1 — 2026-09-21 — initial development snapshot, unreleased

Added source implementations for the Rust capability broker, framed Unix protocol,
registry/policy/reference model, redacted audit, cancellation, CLI/MCP/inspector, declarative
recipes, process plugin SDK/host, semantic Linux backends, narrow GNOME/KWin bridges,
Blender add-on/client and isolated Chromium CDP adapter.

Added Python/JavaScript/native-kernel/browser contract checks, Rust test/fuzz/benchmark
source, build/quality/release definitions, user installation tools, docs and acceptance
traceability. Corrected CLI recipe/manifest serialization, KWin cancelled queue handling,
audit decision labels, fake app-ref validation, configuration section/path consistency and
Chromium snapshot generation handling during review.

Historical state of the initial source snapshot on 2026-09-21: Rust execution, a committed
lockfile, live desktop/Blender evidence and hostile sandbox conformance had not yet been
established, and EIS, PipeWire pixels, persistent portal grants and AT-SPI deltas were unfinished.
Later unreleased commits using the same development version added those implementations and
separate evidence. This historical entry does not describe current support or certify a release;
see `VERIFY.md` and `RELEASE_BLOCKERS.md` for the evidence boundaries.
