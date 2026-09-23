# Platformization integration verification

Baseline: `963f0ceecb22ccfadf66b0937524fb30a6269030`.

This record describes evidence from the platformized integration tree. It does not convert
hosted or cross-target checks into a claim of production macOS support.

## Linux execution

Executed on Linux with Rust/Cargo 1.98.1:

```text
cargo metadata --no-deps --format-version 1                         PASS
cargo fmt --all -- --check                                         PASS
cargo check --locked --workspace --all-targets --all-features      PASS
cargo clippy --locked --workspace --all-targets --all-features
  -- -D warnings                                                   PASS
cargo test --locked --workspace --all-targets --all-features       PASS
cargo test --locked --workspace --all-features --doc               PASS
RUSTDOCFLAGS=-D warnings cargo doc --locked --workspace
  --all-features --no-deps                                         PASS
cargo audit                                                        PASS
cargo deny check                                                   PASS
```
The full workspace test run kept existing live application/display tests ignored when they
required explicit Blender, LibreOffice, KiCad, MLT, Chromium or X11 fixtures. Those are not
silently counted as live platformization evidence.

## Darwin cross-target checks

The Linux host had both Rust targets installed:

```text
aarch64-apple-darwin
x86_64-apple-darwin
```

Shared contracts including protocol, policy, core, federation, Driver SDK/Host and Plugin
SDK/Host typechecked for both Darwin targets. During non-Apple cross-checks,
`platform-macos-sys` deliberately skips compiling its native C object and emits a warning;
that object is compiled and linked only on an Apple host. Therefore these checks prove Rust
target portability, not Apple SDK correctness or runtime behavior.

## Fixes discovered by real Rust gates

The first integration run found and corrected:
- Driver Host conversion from the manifest `PathBuf` system-config destination into the
  platform launch contract's UTF-8 destination, failing closed for non-UTF-8 paths.
- an unsafe `OwnedFd::from_raw_fd` ownership boundary lacking the required safety comment;
- a Rust 1.98 Clippy `is_multiple_of` finding in bounded Mach-O parsing;
- Linux filesystem test placement rejected by `clippy -D warnings`;
- unnecessary lockfile dependency upgrades caused by full offline regeneration. The final
  lockfile preserves external package versions and only records the platformized workspace.

## Native macOS gate

Native Apple SDK/Swift verification is intentionally delegated to
`.github/workflows/platformization-macos.yml` on:

- `macos-15` ARM64;
- `macos-15-intel` x86_64.

The workflow compiles the workspace against the real Apple SDK, builds the Swift native host,
runs noninteractive platform tests, links release host/frontends and executes
`platforms/macos/tests/run-native-smoke.sh`.
A green hosted workflow still does not certify TCC grant/revoke behavior, live AX control,
CGEvent delivery to real applications, ScreenCaptureKit capture after user consent,
multi-display/Retina behavior, installed service lifecycle, signing/notarization or arbitrary
third-party driver/plugin isolation. Those remain dedicated live-Mac acceptance gates.
