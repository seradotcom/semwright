# Development support

Semwright is development software, not a production-supported release. There is no paid
support plan or response-time SLA. Consult [platforms](docs/platforms.md),
[compatibility](docs/compatibility.md) and [release blockers](RELEASE_BLOCKERS.md).

Start with the isolated [quickstart](docs/installation.md). Read
[troubleshooting](docs/troubleshooting.md) before granting broader permissions to solve a
startup error. Missing runtime, consent or sandbox support must not be bypassed.

For public questions and non-sensitive defects, use the repository's
[issue tracker](https://github.com/seradotcom/semwright/issues). Include the exact SHA,
OS/application versions, command, expected/actual result and a minimal synthetic fixture.
Distinguish a fake backend from a real application and an absent tool from a failed test.
Omit tokens, private files, screenshots, clipboard bodies and session tickets.

Use GitHub private vulnerability reporting for security issues, as described in
[SECURITY.md](SECURITY.md). Reporting does not establish a response SLA.
Follow [CONTRIBUTING.md](CONTRIBUTING.md) for focused changes and evidence requirements.
Historical run links retain their original source SHA; new documentation does not renew
a certificate or close R16.

Candidate bundles are runtime packages, not bundled third-party applications or a blanket driver
support certificate. Native CI installation is not physical desktop acceptance. R06/R18 residuals
are explicitly post-v1; no unsigned package is represented as signed/notarized. Public release
still requires independent security review and maintainer authorization under [release policy](docs/release-policy.md).
