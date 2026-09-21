#!/usr/bin/env bash
# Explicit developer action. Does not install Rust, run sudo, or start a daemon.
set -euo pipefail
cd "$(dirname "$0")/../.."
command -v cargo >/dev/null || { echo 'Install an official Rust toolchain yourself; cargo is absent.' >&2; exit 2; }
if [[ -f Cargo.lock ]]; then
    echo 'Cargo.lock already exists; refusing to re-resolve it implicitly.' >&2
    exit 2
fi
cargo generate-lockfile
cargo metadata --locked --format-version 1 > /dev/null
printf '%s\n' 'Dependency resolution completed. Review Cargo.lock, pin rust-toolchain.toml, then run scripts/ci/rust-gates.sh.'
