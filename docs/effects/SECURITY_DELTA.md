# Security delta — Effect Conformance

The evaluator is not authorization, a policy engine, a safety certification or a new trust root. Composition/Broker/Host remain authoritative. R16 remains open.

## Implemented controls
Typed/versioned predicates avoid user eval and arbitrary verifier scripts. Imported observations cannot mint authenticated EvidenceBatch.
Binding/normalization reject wrong owner, request, operation, plan/contract digest, base, provider session/generation, method/source/version/artifact, scope and inconsistent enumeration.
Budgets and trusted observation scope are checked before adapter calls. Required FAIL and UNKNOWN survive aggregation; a client ACK or request echo cannot become independent readback.
The fixed post-write readback-fault cases produce UNKNOWN with zero evidence instead of false PASS.

## Native laboratory
Run 36942492444 at `d2cfd86a2ee064aa5de8f0a8944319edf6dbb060` executed Godot 4.7.2 and Blender 4.5.14 inside disposable GitHub-hosted Linux with Bubblewrap unshare-all, clear environment, read-only source/runtime mounts under the sealed tool namespace, bounded logs/files and deadlines.
The Godot negative observation writer is a separate native process; Blender negative membership also uses a declared external native mutation. Fault injection is separated from product authoring.
This proves only the declared synthetic roots/workflows. It does not enclose an already-open user application or establish machine-wide noninterference.

## Residual limitations
Post-hoc checks detect forbidden effects after execution; OS/Host enforcement is required when prevention is possible.
Fresh reopen proves the observed persistence path, not fsync/atomic crash durability. Quality reports retain Recovery=UNKNOWN and receipts record crash_durability=NOT_TESTED.
Attribution is isolated/ordered only where the harness establishes it; no cross-app causal or rollback guarantee is made.
The Blender authoring candidate at exact head `f492f13a028f781d9ca55631764578f5b327eb1b` classifies unmanaged preservation as Forbidden and represents the whole-scene check with an explicit unmanaged-scene/source-projection address; dedicated authoring run 36942759168 is SUCCESS.
Composition review is complete. Independent R16 review remains an external gate.
