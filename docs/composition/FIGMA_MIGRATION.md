# Figma Composition migration

Public composition spec, plan, result schemas, command descriptors, scopes and risk categories remain unchanged. Native Plugin API measurement and mutations still run through the existing authenticated BridgeHub.

Production authoring now consumes the common server-owned PlanVault, owner binding, cumulative operation/iteration/observation budgets and typed outcome ledger. Plans issued by another host session or changed by a client are rejected even after recalculating a digest. Request arguments cannot set the host owner.

Compatibility: Driver Protocol v2 provides its existing process-local host session; v3/v4 provide Broker session context. Protocol v1 has no session execution context and is no longer accepted for Composition. Use the existing Driver Host with protocol v2 or later. Plans from a previous driver process are invalid after restart; inspect and replan.

Initial node creation capacity is preserved. Additional repair writes consume the original max_mutations budget; repair applications share the original max_iterations and timeout. No failed or unknown mutation refunds capacity or enables automatic retry. Planning is read-only in the native document.

Repair candidates are matched to fresh native validation results at the requested revision and to node identities observed in the original composition. A client-supplied deterministic label, target or repair suggestion is not evidence. Ambiguous, forged and stale candidates are denied.

Figma has no claimed native compare-and-swap or distributed rollback. The adapter revalidates document/session/generation/revision, but external collaborative changes remain best-effort concurrency. A transport failure after dispatch is UNKNOWN and requires observation instead of replay.

Native UI/account acceptance is separate from fixture protocol tests. This document is not evidence that live Figma passed.
