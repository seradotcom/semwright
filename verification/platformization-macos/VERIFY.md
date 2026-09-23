# Platformization + macOS integration verification

Pack baseline:

```text
963f0ceecb22ccfadf66b0937524fb30a6269030
```

Latest integration base after rebasing onto current `origin/main`:

```text
059d997e93fb72c248f8b111929d24acb7f0f7ae
```

The final rebase delta after the full post-registry test pass was documentation-only
(`README.md`, `VERIFY.md`, `RELEASE_BLOCKERS.md`); no Rust source changed.

Branch under verification:

```text
feat/platformization-macos-integration-sol
```

This record distinguishes Linux execution, Darwin cross-checking, native macOS CI and live Mac/TCC acceptance. Those evidence levels are not interchangeable.

## Executed on Linux

The transformed tree was generated from the frozen baseline with the platformization pack's `integration/prepare.py`, then integrated into a dedicated Git worktree. The canonical checkout was not modified.

Executed successfully on the transformed tree:

- `cargo fmt --all -- --check`
- `cargo check --locked --workspace --all-targets --all-features`
- `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings`
- `cargo test --locked --workspace --all-targets --all-features`
- `cargo test --locked --workspace --all-features --doc`
- `RUSTDOCFLAGS="-D warnings" cargo doc --locked --workspace --all-features --no-deps`
- `python3 scripts/verify-source.py`
- Python suite: 113 tests PASS
- Node bridge suite: 20 tests PASS
- `cargo audit`: PASS
- `cargo deny check`: PASS; existing duplicate/license-not-encountered warnings remain warnings
- `scripts/dev/driver-conformance.sh`: PASS with `sandboxed=true`
- `scripts/dev/driver-broker-smoke.sh`: PASS
- `scripts/dev/driver-registry-smoke.sh`: PASS after rebasing onto the Driver Registry/Distribution mainline
- `scripts/dev/fake-smoke.sh`: PASS
- `git diff --check`: PASS

The first full test attempt was interrupted by local disk exhaustion while several agents were building concurrently. No test failure occurred in that attempt. After deleting only this worktree's private target directory and disabling test debug info, the complete workspace test command finished with exit 0.

## Darwin cross-check from Linux

The following Rust-only portable crates type-check for both `aarch64-apple-darwin` and `x86_64-apple-darwin`:

- `semwright-types`
- `semwright-backend-api`
- `semwright-platform-api`
- `semwright-registry`
- `semwright-recipes`

A broader Linux-hosted Darwin check intentionally fails when it reaches `semwright-platform-macos-sys`: its build script requires an authorized Apple SDK on an actual Mac. This is a fail-closed evidence boundary, not a macOS compiler failure.

## Linux preservation

The frozen baseline versions of AT-SPI, bridge, clipboard, Hyprland, portal, Sway and X11 are byte-identical after their move into `platform-linux`. `fake.rs` is byte-identical after its move into `platform-common`. The X11 live test changes only its Linux cfg guard and crate import.

The Linux Driver Host still executes through bubblewrap + Landlock. Real driver conformance and broker smoke passed after the extraction.

## Integration fixes made after applying the pack

- converted the Driver Host system-config destination from `PathBuf` to validated UTF-8 at the platform mount boundary;
- fixed Clippy findings around unsafe documentation, Mach-O alignment idiom and product/test item ordering;
- registered platform crates as version-pinned workspace dependencies instead of wildcard path dependencies;
- preserved the existing registry dependency versions while updating `Cargo.lock` only for the new workspace graph;
- synchronized generated command documentation/contracts;
- documented the cross-platform host architecture and conservative macOS support status.

## Native macOS evidence

PR #29 (`Platformize runtime and add native macOS host foundation`) was merged as
`70c409fc619411b6ecb5fb3f723e27d00cac634e`. Its platformization workflow run
`35815672055` completed successfully on both native Apple architectures:

- ARM64: `macos-15`, job `107036553094`, SUCCESS;
- Intel x86_64: `macos-15-intel`, job `107036552873`, SUCCESS;
- Linux regression: job `107036553060`, SUCCESS.

Run: <https://github.com/seradotcom/semwright/actions/runs/35815672055>

Both macOS jobs:

- cross-checked the Rust-only portable contracts for the opposite Darwin architecture;
- compiled the macOS-capable workspace against the native Apple SDK, explicitly excluding
  the currently Linux-only MLT/KiCad surfaces;
- compiled and linked the Swift/C native host bridge;
- ran the native platform contract tests;
- linked the daemon, CLI, MCP, Plugin Host and Driver Host in release mode;
- executed the noninteractive native smoke.

The native smoke reported on both architectures:

```json
{"native_smoke":"PASS","unique_pasteboard":true,"workspace":true,"getpeereid":true,"live_ax":"NOT_RUN","capture":"NOT_RUN"}
```

The jobs deliberately reported Accessibility consent, live CGEvent control,
ScreenCaptureKit capture, TCC grant/revocation, installed service lifecycle,
codesign/notarization and live multi-display acceptance as `NOT_RUN`.

A follow-up closeout found a Swift concurrency warning in the capture picker caused by
capturing the non-Sendable `SWRequest` in a timer closure. The closeout patch captures
only the immutable request ID and checks cancellation through `CancellationRegistry`.
That change requires a fresh native macOS CI run before this warning can be considered closed.

## Still not certified

Even a green hosted macOS job does **not** certify:

- Accessibility/TCC grant and revocation;
- real AXUIElement interaction with user applications;
- CGEvent input against a live target;
- ScreenCaptureKit capture after user consent;
- multi-display/Retina behaviour;
- installed LaunchAgent/SMAppService lifecycle;
- production code signing/notarization;
- arbitrary third-party driver/plugin isolation on macOS.

Those remain live Mac acceptance gates. The macOS arbitrary-child sandbox path is deliberately fail-closed.
