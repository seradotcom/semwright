# Verification

Native SDK validation is split into focused CI lanes so failures are attributable and expensive work stays off developer workstations.

## GitHub Actions

`.github/workflows/native-sdk.yml` runs for affected pull requests and `main` pushes. It covers repository/package metadata, Rust 1.88 MSRV and the file-backed profile, portable base cooperation tests, Driver/Graph/contracts integration, TypeScript binding and clean external consumers, and clean-room source packaging.

Manual `full_portability=true` expands the portable lane to Ubuntu x64/ARM64, Windows x64/ARM64, and macOS arm64/x64.

`.github/workflows/native-sdk-host.yml` also runs for affected pull requests and `main` pushes and covers the Linux real-Host path with the repository sandbox helper, runtime-tool jobs, adversarial confinement, and Core provider-runtime contracts.

The real Host path exercises CLI/MCP -> daemon -> Broker/Policy -> Driver Host -> native application -> artifact admission/readback -> Graph/Effects. In-process providers or copied admission fixtures do not substitute for that lane.

## CircleCI

CircleCI runs the repository-contract and binding lanes for iteration using the same exact-SHA runner. GitHub Actions remains the final merge gate.

## Test accounting

`scripts/native-sdk/run-suite.py` requires an expected SHA equal to `HEAD`, nonzero test execution, zero skipped/ignored tests, and an unchanged `Cargo.lock`. Reports record source SHA/tree, suite, OS, commands, counts, and workflow/script digests.

## Clean room

`package_clean_room.py` archives the exact source SHA, extracts it into a fresh path, builds the TypeScript binding, and executes clean Rust/TypeScript consumers using only public package surfaces.

## Verified baseline

Full-portability run `37179820287` passed on Ubuntu x64/ARM64, Windows x64/ARM64 and macOS
arm64/x64. The final public integration head `09f71d490ac86f8f8e86dcda6c2552f50c59d487`
passed canonical run `37181039129` and Linux real-Host run `37181039113`.

These are historical exact-SHA baselines. Current changes must pass the affected pull-request lanes;
a green older run is not automatically inherited by a later source revision.
