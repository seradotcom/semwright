# Governance

This archive creates source code, not a legal entity, foundation or existing maintainer
team. The person or organization publishing a public repository must establish ownership,
verified security contacts and a maintainer roster. “Semwright contributors” is a collective
source attribution, not an assertion that named people have accepted responsibility.

The initial governance model is maintainer review with written ADRs for policy, protocol,
reference identity, plugin trust and compatibility changes. A supported release requires
evidence for the release-readiness gates and review of security-sensitive changes by
someone other than the change author where feasible.

Public scope should be driven by reliable narrow commands and demonstrated user value,
not a star target or demo alone. Do not award support labels based solely on compilation,
capability probes or platform existence. Protocol version changes require migration notes
and regression tests. Removing a safety boundary is a breaking design decision, not a
routine compatibility fix.

All source is available for review under the stated licenses. No CLA,
centralized plugin marketplace, usage telemetry or paid hosted backend is required by
this design. A future change to any of those terms must be explicit and documented.

## Initial-v1 admission decision — 2026-10-03

The maintainer authorized Option B: residual R06/R18 physical/interactive certification is post-v1,
not an initial-v1 release prerequisite. Its unexecuted states and withheld support claims remain
visible; no evidence is converted to PASS. This does not waive software defects or security review.
Staging/package validation is separate from publication, which requires a genuine independent
review of the exact candidate, final native validation and explicit maintainer authorization.
Tag pushes do not authorize asset publication. See [release policy](docs/release-policy.md).
No license, branch-protection rule, paid plan or repository visibility is changed by this decision.
