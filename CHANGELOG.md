# Changelog

## Unreleased

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


## 0.9.0-dev.1 — 2026-09-21 — source handoff, unreleased

Added source implementations for the Rust capability broker, framed Unix protocol,
registry/policy/reference model, redacted audit, cancellation, CLI/MCP/inspector, declarative
recipes, process plugin SDK/host, semantic Linux backends, narrow GNOME/KWin bridges,
Blender add-on/client and isolated Chromium CDP adapter.

Added Python/JavaScript/native-kernel/browser contract checks, Rust test/fuzz/benchmark
source, build/quality/release definitions, user installation tools, docs and acceptance
traceability. Corrected CLI recipe/manifest serialization, KWin cancelled queue handling,
audit decision labels, fake app-ref validation, configuration section/path consistency and
Chromium snapshot generation handling during review.

**No Rust compilation, Cargo.lock, binary release, live desktop certification, plugin
sandbox conformance run or live Blender result exists for this version.** EIS, PipeWire
pixels, persistent portal grants and AT-SPI deltas remain unimplemented. See VERIFY.md and
RELEASE_BLOCKERS.md. No production-ready claim or fake green CI badge is made.
