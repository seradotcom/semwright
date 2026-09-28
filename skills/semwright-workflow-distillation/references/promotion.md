# Promotion

Current promotion gates require both:

- static verification; and
- at least one successful live replay.

Promotion registers a capability named `recipe.<slug>.run` with Recipe provenance in the ordinary capability catalog. Execution still enters the Broker and policy boundary; promotion does not copy authority from the source traces.

When a promoted capability drifts because an underlying descriptor changed, treat it as stale evidence. Re-verify/recompile rather than suppressing drift.

Progressive compilation should shrink the Skill over time: keep judgment about when/why/exceptions in the Skill and move stable deterministic mechanics into the promoted capability.
