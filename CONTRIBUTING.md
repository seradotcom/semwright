# Contributing

Start with [development](docs/development.md), [architecture](docs/architecture.md),
[verification](VERIFY.md) and [release blockers](RELEASE_BLOCKERS.md). This source archive
has no established public issue tracker yet; the eventual publisher must add the actual
repository/reporting channel instead of leaving an invented URL.

Keep command schemas and policy authoritative. A new frontend must use the broker. A
backend must not interpret model text as code, silently choose ambiguous targets, migrate
an old ref, retry an uncertain effect, or activate a more privileged fallback. Add a
regression fixture for correctness/security changes. Test application-native functionality
without borrowing credentials or normal user profiles.

Before submitting, format/lint/test the affected workspace and report exact commands,
counts, failures, unexecuted tests and live environment versions. Do not label a fake test
as live desktop validation, alter verification logs, or remove a failing gate to claim a
release. Generated command docs must be regenerated with scripts/sync-contracts.py.

Keep original requirements in docs/requirements unchanged for traceability. Record design
changes in an ADR. Ordinary Rust code is dual MIT OR Apache-2.0; by contributing original
code, you agree to the same licensing. Third-party code/assets require preserved notices
and dependency review. Avoid adding dependencies without a concrete capability reason.

Security reports should follow SECURITY.md and remain private until an actual reporting
channel is established. No public credentials, browser cookies, clipboard contents,
private screenshots or copied personal application data belong in test fixtures.
