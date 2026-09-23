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

Native ARM64 and Intel jobs are defined in `.github/workflows/platformization-macos.yml`. They are required to:

- compile the macOS-capable workspace against a real Apple SDK, explicitly excluding the currently Linux-only `semwright-mlt-video-driver`;
- compile/link the Swift/C native host bridge;
- run native platform contract tests;
- link the daemon and frontends;
- execute the noninteractive native smoke.

This section must be updated with exact run URLs/results after the branch is pushed.

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
