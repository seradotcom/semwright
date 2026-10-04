# Compatibility

Compatibility is reported by evidence level, not one universal supported flag.

| Surface | Linux x64 | Linux arm64 | Windows x64 | Windows ARM64 | macOS arm64 | macOS x64 |
|---|---|---|---|---|---|---|
| base cooperation library | Actions | Actions | Actions | Actions | Actions | Actions |
| Rust/Graph/contracts portability | Actions | Actions | Actions | Actions | Actions | Actions |
| real daemon/Broker/Driver Host sandbox E2E | accepted profile only after exact-SHA Host PASS | not claimed | not claimed | not claimed | not claimed | not claimed |
| materialized Node bridge | Linux Host profile | not physically certified | not claimed | not claimed | not claimed | not claimed |

The full Actions portability run at source SHA `5aa6eafd97946ee7cddfe82f0848b5033acbc086` succeeded on `ubuntu-24.04`, `ubuntu-24.04-arm`, `windows-2025`, `windows-11-arm`, `macos-15`, and `macos-15-intel`. Later source changes require their own exact-SHA rerun before inheriting final closure.

The workspace MSRV remains Rust 1.88. Cross-compilation is not counted as native execution. Host certification is not inferred from library portability.

Unknown protocol/descriptor drift is rejected rather than approximated. Driver interfaces such as native refs, Host tools and cancellation are declared only when used.
