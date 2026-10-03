# Canonical Native SDK integration

Status: staged implementation, not a release or acceptance claim.

The application owns its domain model, storage, revisions and transactions. The
Native SDK provides optional cooperation interfaces and adapts them to the
existing Driver SDK. Broker/Policy, Driver Host, Composition, Project Graph and
Effects retain their existing authorities.

The received 0.3.0 source is a REBUILT private source cut without an explicit
license grant for public redistribution. No license is inferred from its Core
dependencies. This public checkpoint contains independently written audit and
integration metadata, not the received implementation. Derived source and its
validation must remain private until the owner resolves that gate.

Read DISCOVERY_REPORT.md, SOURCE_LOCK.json, ZIP_MIGRATION.md,
CANONICAL_MAPPING.md, CI_PLAN.md and ACCEPTANCE_MATRIX.json. A successful audit
job does not mean that a native application, sandbox or daemon was tested.

No merge, tag, release, registry publication or license change is part of this work.
