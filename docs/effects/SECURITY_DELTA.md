# Security delta — F candidate

The effect evaluator is not an authority, safety certification or authorization service. R16 remains open. The generic library evaluates data; current native examples are test harnesses, not public Broker capabilities.

## Additions
Closed/versioned predicates avoid user eval, arbitrary methods and unchecked paths. A owns canonicalization, owner/base/plan types, verdicts, PlanVault and convergence budgets. Imported observations cannot construct an authenticated EvidenceBatch. Compiled adapters must obtain identity from the real execution channel, never from request metadata.

Binding and normalization reject wrong owner/request/operation/plan/contract, stale or unknown bases, wrong provider session/generation, changed channel identity, unsupported evidence class, substituted method/source/artifact, incomplete scopes and inconsistent enumeration. Observer count limits and trusted scope are checked before adapter calls. Host adapters still enforce elapsed-time, memory and OS budgets; this synchronous library cannot preempt a native getter.

`validate_plan` checks consistency with A's actual PreparedPlan, including the pinned effects.contract dependency and unchanged budget. It does not reserve or execute a plan. The existing Broker/PlanVault path must admit every observer/render/open operation and every repair.

## Native laboratory boundary
The scripts use GitHub-hosted disposable roots, bubblewrap unshare-all, clean environments, no inherited GitHub credentials in app subprocesses, read-only source/runtime mounts, bounded logs/files and explicit deadlines. This configuration is implemented but native success has not yet been confirmed. It does not enclose the user's live creative application or prove general project safety.

Godot test input is explicitly synthetic in-memory scene state; SaveOps performs scene-only persistence and ReadbackOps observes it. Blender authoring/export/save/open calls use the fixed product Commands adapter. Direct native writes occur only in declared fixture setup or labeled fault injection. No result is attributed to a full Broker E2E.

The Rust native example trusts only its own fixed subprocess invocation and independently reads native output; it is not an API for importing arbitrary client JSON. Trusted in-process adapter implementations remain part of the TCB. A malicious registered adapter is not made trustworthy by this crate's types.

## Residual limitations
Post-hoc forbidden-effect detection cannot prevent a write. Bounded inventories establish only the declared synthetic root and budgets, not machine-wide noninterference. Happy-path fresh reopen does not establish fsync, atomicity or crash durability. Causality is not inferred from equal/different bytes. There is no cross-app rollback or exactly-once guarantee.

Outstanding: current-SHA native/negative gates, production D/E adapter admission, owner A review, targeted mutation execution, portability, audit/license gates and independent security review.
