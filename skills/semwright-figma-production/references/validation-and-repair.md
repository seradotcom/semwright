# Validation and repair

Treat validation evidence in distinct classes:

- **DETERMINISTIC**: machine-verifiable facts such as clipping, bounds overflow, declared gap/aspect-ratio violations, missing required bindings, stale refs or broken relationships.
- **HEURISTIC**: useful signals such as spacing irregularity or density; do not present these as objective truth.
- **AESTHETIC_ASSIST**: hierarchy, balance, brand fit and taste; these remain model/human judgment.

A finding never grants mutation authority. For deterministic unambiguous cases, request a bounded repair plan, review it, apply it through Broker policy, then freshly measure and validate again.

Stop on ambiguous repair, stale state, broader required authority, unexpected severity worsening, exhausted iteration/mutation budgets or lack of deterministic progress.
