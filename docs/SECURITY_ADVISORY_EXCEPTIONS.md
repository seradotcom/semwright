# Reviewed RustSec maintenance advisories

Reviewed: 2026-10-31. This file documents the four **informational, unmaintained** advisories temporarily excluded from the failing-on-warnings dependency checks. No known vulnerability is excepted. All seven vulnerabilities found in the 2026-10-26 main audit are addressed by patch-level Cargo.lock upgrades: ctutils 0.4.4, rustls 0.23.46, rustls-webpki 0.103.16, webpki-roots 1.0.10 and tokio 1.53.3 (which also addresses its previous unsound warning).

The project declares Rust 1.88 as its minimum supported version. The tracked dependencies below require a deliberate, tested migration, not a blanket audit disablement.

| RustSec | Dependency | Reason and removal criterion |
| --- | --- | --- |
| RUSTSEC-2024-0449 | unsafe-libyaml 0.2.11 via serde_yaml_ng | Bounded recipe and skill frontmatter parsers use the old library transitively. Replace with a maintained safe-Rust YAML parser, including compatible validation and negative-test coverage, before removing this exception. |
| RUSTSEC-2026-0370 | zvariant 5.15 via zbus 5 | zbus 6 merges the wire types but requires Rust 1.89. Move the D-Bus integration to zbus 6 when the declared MSRV and Linux build/test coverage are migrated. |
| RUSTSEC-2026-0371 | zbus_names 4.3 via zbus 5 | Same zbus 6 migration and MSRV prerequisite. |
| RUSTSEC-2026-0372 | zvariant_derive 5.15 via zbus 5 | Same zbus 6 migration and MSRV prerequisite. |

These exceptions are intentionally **advisory-ID-specific**, listed in both `deny.toml` and the security workflow's `cargo audit --deny warnings` command. The audit must continue to fail for any unexcepted warnings, yanked dependencies or vulnerabilities, including future advisories affecting the same crates.

Review these exceptions before the next Semwright toolchain/MSRV upgrade; do not treat a green CI result as proof that an unmaintained dependency is safe. Upgrades of the YAML parser or D-Bus stack require their own conformance tests and cannot be silently folded into a lockfile security patch.
