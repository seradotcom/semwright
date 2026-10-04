# Governance

Semwright uses maintainer-led governance. Repository maintainers are responsible for project
stewardship, release decisions, security-report routing and the compatibility policy for public
interfaces. Maintainer decisions should be recorded in pull requests, ADRs or release documentation
when they affect authority boundaries, protocols, reference identity, package trust or support.

Security-sensitive changes should be reviewed by someone other than the change author where
practical. A supported release requires the evidence defined by the release policy; passing a build
or capability probe does not by itself establish platform or application support. Protocol changes
require migration notes and regression coverage. Removing a safety boundary is a breaking design
decision, not routine compatibility work.

Public scope is driven by demonstrated user value and reproducible behavior. Support claims must be
bound to the platform, application version and workflow that were actually exercised. Historical
evidence stays historical rather than being promoted to a newer commit without revalidation.

Semwright core source is available under MIT OR Apache-2.0. The KiCad integration retains its
separate GPL-3.0-or-later boundary and notices. Changes to contribution terms, telemetry, hosted
services, marketplaces or licensing must be explicit and reviewed rather than inferred from project
growth.

## Initial-v1 admission decision — 2026-10-03

The maintainer authorized Option B: residual R06/R18 physical/interactive certification is post-v1,
not an initial-v1 release prerequisite. Its unexecuted states and withheld support claims remain
visible; no evidence is converted to PASS. This does not waive software defects or security review.
Staging/package validation is separate from publication, which requires a genuine independent
review of the exact candidate, final native validation and explicit maintainer authorization.
Tag pushes do not authorize asset publication. See [release policy](docs/release-policy.md).
No license, branch-protection rule, paid plan or repository visibility is changed by this decision.
