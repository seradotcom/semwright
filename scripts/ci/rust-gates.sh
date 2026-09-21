#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
command -v cargo >/dev/null || { echo 'BLOCKED: cargo is required' >&2; exit 2; }
test -f Cargo.lock || { echo 'BLOCKED: Cargo.lock is absent. Resolve and review it before running release gates.' >&2; exit 2; }
cargo fmt --all -- --check
cargo check --locked --workspace --all-targets --all-features
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-targets --all-features
cargo test --locked --workspace --doc
RUSTDOCFLAGS='-D warnings' cargo doc --locked --workspace --no-deps
cargo build --locked --workspace --release
cargo audit --deny warnings
cargo deny check
