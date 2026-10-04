# Compatibility

Compatibility is reported by evidence level, not one universal supported flag.

| Surface | Linux x64 | Linux arm64 | Windows x64 | Windows ARM64 | macOS arm64 | macOS x64 |
|---|---|---|---|---|---|---|
| base cooperation library | PASS | PASS | PASS | PASS | PASS | PASS |
| portable Rust cooperation contracts | PASS | PASS | PASS | PASS | PASS | PASS |
| Driver/Graph/contracts integration | PASS on hosted Linux integration lane | portable contract surface only | portable contract surface only | portable contract surface only | portable contract surface only | portable contract surface only |
| real daemon/Broker/Driver Host sandbox E2E | PASS accepted Linux Host profile | not claimed | not claimed | not claimed | not claimed | not claimed |
| materialized Node bridge | PASS Linux Host profile | not physically certified | not claimed | not claimed | not claimed | not claimed |

The public six-platform baseline is Actions run `37179820287` at source
`d4c7a7795a8a529b3fb170c52564788579ce19e0`: Ubuntu x64/ARM64, Windows x64/ARM64 and macOS
arm64/x64 all passed. The final public integration head
`09f71d490ac86f8f8e86dcda6c2552f50c59d487` then passed the normal canonical suite in run
`37181039129` and the Linux real-Host suite in run `37181039113`.

Those runs are exact-SHA evidence, not permanent inheritance for later source changes. The Native
SDK workflows run on affected pull requests and `main` pushes so changes to the SDK or its
canonical dependencies receive fresh evidence.

The workspace MSRV remains Rust 1.88. Cross-compilation is not counted as native execution, and
portable cooperation tests do not imply real Host certification on every operating system.

Unknown protocol/descriptor drift is rejected rather than approximated. Driver interfaces such as
native refs, Host tools and cancellation are declared only when used.
