#!/bin/bash
set -euo pipefail
[[ $(uname -s) == Darwin ]] || { echo 'BLOCKED: this verifier needs macOS and an Apple SDK' >&2; exit 77; }
root=$(cd "$(dirname "$0")/../../.." && pwd)
cd "$root"
# Never change toolchain pins or dependency versions to make this check pass.
rustup show active-toolchain
rustup target add aarch64-apple-darwin x86_64-apple-darwin
for target in aarch64-apple-darwin x86_64-apple-darwin; do
 cargo check --locked --target "$target" -p semwright-types -p semwright-protocol \
   -p semwright-backend-api -p semwright-platform-api -p semwright-registry \
   -p semwright-policy -p semwright-recipes -p semwright-core -p semwright-federation \
   -p semwright-driver-sdk -p semwright-driver-host -p semwright-plugin-host
done
