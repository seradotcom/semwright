# Compatibility

Compatibility is reported by evidence level, not one universal supported flag.

| Surface | Linux x64 | Linux arm64 | Windows x64 | Windows ARM64 | macOS arm64 | macOS x64 |
|---|---|---|---|---|---|---|
| base cooperation library | native CI | native CI | native CI | native CI | native CI | native CI |
| portable cooperation contract | PASS milestone | PASS milestone | PASS milestone | PASS milestone | PASS milestone | PASS milestone |
| real daemon/Broker/Driver Host sandbox E2E | PASS | not claimed | not claimed | not claimed | not claimed | not claimed |
| materialized Node bridge | Linux Host profile | not certified | not claimed | not claimed | not claimed | not claimed |

Public six-runner portability run `37179820287` at source
`d4c7a7795a8a529b3fb170c52564788579ce19e0` passed on `ubuntu-24.04`,
`ubuntu-24.04-arm`, `windows-2025`, `windows-11-arm`, `macos-15`, and
`macos-15-intel`.

The final PR #213 public head `09f71d490ac86f8f8e86dcda6c2552f50c59d487` subsequently passed
canonical Native SDK run `37181039129` and real-Host run `37181039113`. The canonical run
covered metadata, the Rust 1.88 file-backed profile, Driver/Graph/contracts, TypeScript
binding/clean consumers, package clean-room and portable Ubuntu. The Host run covered the real
Linux CLI/MCP -> daemon -> Broker/Policy -> Driver Host -> native application path.

The workspace MSRV is Rust 1.88. Cross-compilation is not counted as native execution, and
library portability is not Host or application certification. Windows/macOS application-specific
integration evidence is tracked separately from this SDK portability matrix.

Unknown protocol/descriptor drift is rejected rather than approximated. Driver interfaces such
as native refs, Host tools and cancellation are declared only when used.
