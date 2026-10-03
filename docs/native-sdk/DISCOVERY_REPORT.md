# N0 discovery

## Immutable baseline

The worktree was created from live origin/main
`4f121b3cd1d469c556adfa54159878dc96022148`, independently matched against
`git ls-remote origin refs/heads/main`. The existing checkout was on a different
branch and was not treated as main. The repository is public. Other worktrees,
installation/staging PRs and branding changes are out of scope.

## Received material

SOURCE_LOCK.json inventories every member of the two source/evidence ZIPs and
the instruction archive: 61 source, 51 validation and 9 instruction files.
All 118 non-checksum members matched their internal SHA-256 manifests. Archive
CRC, duplicate names, path traversal, symlinks and UTF-8 were checked without
executing archive scripts. These checks identify bytes, not authorship.

The SDK used historical Core pin
`6491c0d838fa066938a494524d69ed507aa0dbe8`; it is not the integration baseline.
REBUILT provenance is retained. The source archive's aggregate source identity
is `4fbe99bdb5fd330273a855d139f4c36113f7ec54bb132988cd19247fb208158d`.
Original archives and logs are retained privately, unchanged.

## Findings affecting the port

1. The central Model contract owns a JSON document, filesystem and revision
   counter. It is a useful optional profile, not a general cooperation API.
2. Mutation descriptors overstate universal reversibility and idempotency.
   Per-operation canonical descriptors and explicit guarantees are required.
3. The recovery journal preserves uncertainty but reaches a lifetime ceiling
   of 512 keys. Raising the constant is not sustainable retention.
4. File locking uses std::fs::File::try_lock, beyond workspace MSRV 1.88.
5. Historical artifacts and verifier reports do not prove real Driver Host,
   daemon/MCP execution or authenticated Graph/Effects admission.
6. The file profile is Unix-only and protected artifact verification is Linux
   specific. Those limits must not become universal SDK requirements.
7. Path-based document I/O needs a separate secure-filesystem review. Existing
   fail-closed behavior must not be weakened to get a sandbox test passing.
8. The SDK source has no resolved public redistribution license. Upstream
   license notices and Cargo publish=false do not supply that permission.

## Historical evidence, not current acceptance

The sidecar reports 72 Rust test executions: 54 unique tests and 18 repeats.
Packaging/helper/preparation tests, clean consumers and direct package
inspection have separate scopes. The provider tests are in-process; they are
not Driver Host evidence. The historical UNKNOWN/unknown assertion correction
remains historical evidence, not a current test result.

NATIVE_HOST, daemon/MCP, current-SHA portability, private publication and the
Platform adapter have no new acceptance here. Every acceptance row starts
NOT_RUN. Review completeness is recorded separately from structural checks;
an inventory is not an assertion that every behavior has been proven.
