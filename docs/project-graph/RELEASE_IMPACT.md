# Release impact — role C

Status: development component in draft PR #173. No release/tag/main merge, production rollout, native readiness or R16 closure is requested.

The workspace gains `semwright-project-graph`; its default surface is the portable identity/model/query/rebuild library. Optional `store` adds pinned rusqlite 0.40.2 with bundled SQLite and the existing platform-services boundary. The remote-resolved lock introduced only the documented store dependency closure, without upgrading unrelated workspace packages. Registry and Skills are development dependencies for actual package/conformance tests, not a second runtime authority.

A C0 is consumed unchanged. The generic ScopedRoot trait receives an additive object-safe observation method with a conservative default Unsupported implementation. Linux implements bounded native instance observation. Existing scoped read/write methods are unchanged. Rust consumers should rebuild coherently from the integrated SHA; this is not a binary-ABI compatibility promise.

Private graph schema 1 is new. No application files or existing workflow stores are migrated. The event journal preserves revisions/activities and allows explicit index reconstruction; unrecognized schema versions fail closed. Portable imports never acquire local owner identity, trusted receipts, locators or CURRENT state.

The continuity Skill teaches discovery, evidence limits and the runtime blocker. Its generated lock binds real discovery descriptors. Package/conformance evidence is separate from executing project operations. No new Broker/CLI/MCP graph commands are registered in this branch because global adapter wiring was tool-blocked. The adapter remains a review draft, not shipped runtime behavior.

Required remaining release gates include reviewed Broker/native adapters, a durable authenticated-principal handoff into Broker context, wiring the implemented external-intent ledger around real provider dispatch/reconciliation, D/E Blender→GLB→Godot and A/B AV/audio receipt flows, clean-room native installation, applicable non-Linux acceptance, dependency audit/license gates and full exact-candidate regression. Fuzz smoke and an internal adversarial agent cannot replace those gates or R16.
