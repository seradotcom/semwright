# Security policy

## Current status

This is **development software with executed CI and live integration evidence**, not a security-reviewed automation product.
There is no supported production version and no paid security response commitment.
Do not attach it to a credential-rich desktop on the basis of staging or CI alone. Independent
security review remains required before public release; physical/interactive support claims require
their separate evidence. See [release policy](docs/release-policy.md).

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

The public repository is `seradotcom/semwright` on GitHub, and GitHub private
vulnerability reporting is enabled. Report security issues through the repository's
private vulnerability-reporting flow; do not open a public issue containing credentials,
private artifacts, or an exploit against a third party. This is a reporting channel, not
a promise of production support or a response-time SLA.

Provide the affected commit, minimal local fixture, precise capability/profile, expected
boundary, actual outcome and redacted environment. Do not include live credentials,
session tickets, cookies, raw clipboard contents, private screenshots or unrelated files.
Tests must target a machine/account you own or are explicitly authorized to assess.

See [threat model](docs/security.md), [permissions](docs/permissions.md),
[independent review packet](docs/security-review.md), [release blockers](RELEASE_BLOCKERS.md)
and [verification](VERIFY.md).

## R16 review and authority limits

The recorded R review targets `6491c0d838fa066938a494524d69ed507aa0dbe8`; its source observations,
changes and subsequent verification artifacts are separately identified in
[verification/r16-closeout](verification/r16-closeout/README.md). This repository-scoped
review is not an external audit. R16 is CLOSED after its separately recorded revalidation,
but that does not satisfy the independent public-release security review. A successful CI workflow is not
a security approval, especially when its platform jobs were skipped.

Local peer checks do not isolate a separately hostile same-user process. Driver sandboxing
does not sandbox an existing external application. Stored project identities, generated
Skills and native readback do not create authority beyond the broker's explicit grants.
See [the threat model](docs/security.md), [permissions](docs/permissions.md) and
[platform limitations](docs/platforms.md) before exposing a real desktop.

The private vulnerability-reporting API was observed enabled during this review. No public
security issue, reporting setting or disclosure policy was changed. Preserve sensitive
reproduction material privately; public review summaries must omit secrets and private data.
