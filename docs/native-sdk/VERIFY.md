# Verification

Native SDK validation is split into focused CI lanes so failures are attributable and expensive work stays off developer workstations.

## GitHub Actions

`.github/workflows/native-sdk.yml` covers repository/package metadata, Rust 1.88 MSRV and the file-backed profile, portable base cooperation tests, Driver/Graph/contracts integration, TypeScript binding and clean external consumers, and clean-room source packaging.

Manual `full_portability=true` expands the portable lane to Ubuntu x64/ARM64, Windows x64/ARM64, and macOS arm64/x64.

`.github/workflows/native-sdk-host.yml` covers the Linux real-Host path with the repository sandbox helper, runtime-tool jobs, adversarial confinement, and Core provider-runtime contracts.

The real Host path exercises CLI/MCP -> daemon -> Broker/Policy -> Driver Host -> native application -> artifact admission/readback -> Graph/Effects. In-process providers or copied admission fixtures do not substitute for that lane.

## CircleCI

CircleCI runs the repository-contract and binding lanes for iteration using the same exact-SHA runner. GitHub Actions remains the final merge gate.

## Test accounting

`scripts/native-sdk/run-suite.py` requires an expected SHA equal to `HEAD`, nonzero test execution, zero skipped/ignored tests, and an unchanged `Cargo.lock`. Reports record source SHA/tree, suite, OS, commands, counts, and workflow/script digests.

## Clean room

`package_clean_room.py` archives the exact source SHA, extracts it into a fresh path, builds the TypeScript binding, and executes clean Rust/TypeScript consumers using only public package surfaces.

## Public integration evidence

The Native SDK was merged through PR #213. On the final public branch head `09f71d490ac86f8f8e86dcda6c2552f50c59d487`, canonical run `37181039129` passed metadata, file-profile/MSRV, portable Ubuntu, Driver/Graph/contracts, TypeScript binding/clean consumers and package clean-room jobs; real-Host run `37181039113` also passed.

The six-runner portability milestone is public run `37179820287` at `d4c7a7795a8a529b3fb170c52564788579ce19e0`: Ubuntu x64/ARM64, Windows x64/ARM64 and macOS arm64/x64 all passed. Later source edits on the PR head were rerun through the affected canonical and real-Host lanes. Treat these as exact-source records, not a promise that every application or Host profile is certified on every operating system.

## Verified baseline

The canonical implementation has been exercised on Rust 1.88 across Ubuntu x64/ARM64, Windows x64/ARM64, and macOS arm64/x64, plus a Linux real-Host lane. Future changes must re-run the affected exact-SHA lanes before merge.
