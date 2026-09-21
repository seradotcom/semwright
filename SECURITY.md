# Security policy

## Current status

This is **uncompiled development source**, not a security-reviewed automation product.
There is no supported production version and no paid security response commitment.
Do not attach it to a credential-rich desktop until the release blockers are closed.

The intended boundary is least-privilege **mediated commands**: a broker policy controls
all frontends, references identify objects, mutations do not silently retry, and plugin
processes receive narrow filesystem/environment/network rights. No root daemon, setuid
helper, unrestricted shell, eval command, default credential access or public TCP listener
is shipped.

## What it does not protect against

A malicious process with the same Unix UID can access the user's data and may impersonate
local services; Unix socket UID checks do not defeat that attacker. An agent separately
given a full shell or another unrestricted desktop-control tool can bypass this broker.
Policy is not a universal solution to prompt injection. User-visible labels and page text
are untrusted data, not instructions; observe access itself can reveal sensitive labels.

App-native adapters call existing application processes that retain the user's ordinary
privileges. Blender path APIs are not FD-relative. Chromium top-level origin restrictions
are not a firewall for redirects/subresources or JavaScript running normally in pages.
No claim of complete isolation or information-flow security is made.

## Reporting vulnerabilities

No public repository/contact endpoint is established by this source archive. Report
privately to the person or organization that supplied your copy; do not invent a public
issue containing credentials or an exploit against a third party. Once published, the
maintainer must enable GitHub private vulnerability reporting and update this section
with the actual verified reporting channel before a supported release.

Provide the affected commit, minimal local fixture, precise capability/profile, expected
boundary, actual outcome and redacted environment. Do not include live credentials,
session tickets, cookies, raw clipboard contents, private screenshots or unrelated files.
Tests must target a machine/account you own or are explicitly authorized to assess.

See [threat model](docs/security.md), [permissions](docs/permissions.md),
[release blockers](RELEASE_BLOCKERS.md) and [verification](VERIFY.md).
