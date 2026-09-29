# Consumer handoff — E on A/C/F

E consumes A C0 `26602e4b25929be869d69ef28fef4dd9713180d7`, C P0 publication `6ee52b428310370d3ad438a13964086a63f48367`, F head `42204ac6a6f3c66ba66de5adfe689d8633bb7c74`, and the GLB work now merged from PR #154. This is source consumption, not automatic certification of those heads. F's exact-SHA run 36504224825 failed a Clippy naming lint in F's own test; E records that independently of its API integration.

## F effect evidence

Every E `PreparedPlan` now pins `dependencies["effects.contract"]` to F's actual `EffectContract::digest`, and its required rules are exactly the contract's required rules. The trusted Rust adapter implements F's `EvidenceAdapter`; it derives principal/session/generation from the Driver execution context and native post-state, calls `collect`, then F's `evaluate`. No client JSON constructs `EvidenceBatch`.

The first contract checks complete bounded native source readback of the authored plan. Create workflows also check preservation of the whole-scene projection after excluding the newly managed island. These checks do not claim global filesystem/process noninterference, mesh self-intersection proof, or aesthetic quality. Missing global readback becomes UNKNOWN rather than rollback or PASS.

## C Project Graph

E uses C's actual `ProjectId`, `LogicalAssetId`, `AssetRevision`, `RevisionPin`, `OperationIdentity`, `ExecutionReceipt` and `ReceiptAdapter`. `graph_receipt_candidate` requires host-owned C IDs and exact descriptor/runtime digests. Local spec aliases and Blender island markers never become logical IDs.

The candidate records plan, parameter, descriptor, runtime and F contract determinants. Only C's registered `ReceiptAdapter::admit` can promote it to `AdmittedReceipt`. E does not expose receipt admission as a capability. Current coverage remains incomplete until export, handoff/import, external assets and remaining native dependencies are represented as separate C activities/revisions.

## D Godot

The existing `driver.blender.export.glb` remains the only GLB exporter. E does not write `.tscn`, call private Godot helpers or add `send_to_godot`. D must consume the exported artifact through its public artifact/import boundary and produce a distinct Godot import/verification activity. That native Blender→GLB→Godot roundtrip is still open for E11.
