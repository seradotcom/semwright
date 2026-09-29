---
name: semwright-project-continuity
description: Use when continuing a native creative project, identifying resources, checking drift and dependencies, or deciding whether an output may be reused. Preserve evidence scope and stop when Project Graph runtime routes are unavailable.
---

# Project continuity

First discover available capabilities and describe the exact operation before using it. The included requirements and examples cover the real catalog discovery commands, not permission to modify a project.

**Current integration boundary:** this package accompanies the C Project Graph library. Its Broker adapter is still a review draft; graph routes are not registered by this branch. Discovering no suitable graph route is a product blocker, not a reason to write the private database, fabricate a receipt, call a private helper or substitute shell automation. Static Skill/package validation does not close native continuity acceptance.

When the installed catalog exposes the complete authorized graph path, follow this sequence:

1. Register or discover the logical resource under an explicit project/root grant. Do not equate a path, matching name, digest or old session ref with persistent identity.
2. Reconcile through the authorized native observer. Reacquire current refs and record observation scope/time, base, generation, digest method and unknown dependencies. Report “current according to the last reconcile” separately from “verified now.”
3. Inspect dependencies, derivations and impact. Keep known affected resources, possible impacts, unknown frontier and traversal truncation separate. An absent edge does not establish independence.
4. Review the typed rebuild proposal and blockers. Use the existing Broker/controller and current catalog bindings for preparation and execution. The proposal is not a shell program, grant, permission to overwrite divergence or another scheduler.
5. Verify required output predicates using the same base/artifact, persist actual receipts and refresh knowledge. Preserve required UNKNOWN/FAIL. Do not infer success from a saved filename or successful store tests.

See [states](references/states.md). This Skill never promises silent synchronization or work in the background.
