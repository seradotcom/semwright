# Dependency review — first executable baseline

The unmodified handoff resolved to Ratatui 0.29.0, `paste` 1.0.15 and `lru` 0.12.5.
The actual audit reported RUSTSEC-2024-0436, RUSTSEC-2026-0002 and RUSTSEC-2026-0253.
The inspector was upgraded to Ratatui 0.30.2 and Crossterm 0.29.0; the new graph resolves
`lru` 0.18.4 and no longer includes `paste`. No advisory was added to the ignore list.

Sources: https://rustsec.org/advisories/RUSTSEC-2024-0436.html,
https://rustsec.org/advisories/RUSTSEC-2026-0002.html,
https://rustsec.org/advisories/RUSTSEC-2026-0253.html.

`cargo audit --deny warnings` passed (`audit-third.log`).
`cargo deny --locked check` passed (`deny-second.log`). Duplicate dependency versions
remain warnings under the existing policy, not silently suppressed errors.
The permissive MIT-0 identifier used by `borrow-or-share` was added explicitly to the
license allowlist. Project licensing remains MIT OR Apache-2.0. Internal workspace
path dependencies now require the exact development version rather than wildcards.

The official rmcp 3.4.0 SDK client feature was enabled. Two actual stdio binary E2E
scenarios passed: modern 2026-07-28 discovery and legacy initialization, each communicating
through the real Unix IPC broker. They check discovery, permissions, execution, audit,
unknown arguments and stale identity. Source: https://github.com/modelcontextprotocol/rust-sdk.

This is a dependency/toolchain review and execution record, not an independent security audit.
