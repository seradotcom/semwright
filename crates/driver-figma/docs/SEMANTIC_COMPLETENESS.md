# Figma semantic completeness

Semwright calls this driver **semantically complete for a pinned public Figma API baseline** when every public surface in that baseline is inventoried and has an explicit classification. Completeness is about semantic coverage, not about exposing one tool per Figma property and not about arbitrary JavaScript execution.

Current pinned baselines:

- Plugin API typings: `@figma/plugin-typings 1.139.0`
- REST OpenAPI: `figma/rest-api-spec@04fbbc719706e986fc79f3050d3e068e118275d9`

The coverage compiler inventories global interfaces, auxiliary interfaces and every public SceneNode member. CI fails when a new public method is left unclassified, when a mapped capability does not exist, when scene-node members fall out of coverage, or when an unresolved policy/gap classification appears.

## Semantic classifications

A public API member may be:

- backed by a typed Semwright capability;
- covered by the generic allowlisted property read/write surface;
- internal runtime metadata/lifecycle needed to implement the bridge;
- semantically superseded by a safer Semwright operation;
- delegated to another Semwright provider when it is not actually Figma document semantics;
- restricted by Figma itself to partner/widget contexts;
- an internal secret-composition primitive that must never be returned to the agent.
Examples:

Host object references are never treated as generic JSON writes. Properties such as `InstanceNode.mainComponent` and `StickableMixin.stuckTo` are removed from the generic write allowlist and routed through explicit ref-aware capabilities; `mainComponent` is also removed from generic reads under dynamic-page mode and read through the async instance inspection path.

- `PluginAPI.createImageAsync` is superseded by artifact-backed `image.create`; the driver does not perform arbitrary remote URL fetches inside Figma.
- `PluginAPI.openExternal` is delegated to Semwright's browser/navigation authority instead of letting document content trigger navigation.
- `PaymentsAPI.getPluginPaymentTokenAsync` is classified `INTERNAL_SECRET_COMPOSITION`; payment identity tokens are not agent-visible.
- partner-only `setDevResourcePreviewAsync` and widget-context-only mutation APIs remain explicitly marked as upstream restrictions rather than Semwright gaps.

## Current coverage

The generated Plugin API inventory currently contains 18 global interfaces, 14 auxiliary interfaces, 34 scene-node types, 213 global members, 49 auxiliary method entries and 3,699 scene-node members. All 3,699 scene-node members are classified and there are zero unmapped method names.

The REST inventory maps all 54 operationIds in the pinned official OpenAPI snapshot, plus one documented semantic discovery helper. `cloud.status` reports whether the owner-provisioned credential transport is available.

The advertised agent-facing catalog currently contains 390 capabilities: 331 Plugin API operations, 56 cloud operations and three local driver/session operations. The additional product-semantic operations include typed Motion orchestration (`motion.apply`, presets and stagger) and native Slot conversion/settings, while preserving the lower-level official Plugin API primitives.

## What completeness does not mean

It does not mean every route has been exercised against a real production Figma account. Fake-runtime, Driver Host and hosted CI prove protocol and implementation contracts; protected real-Figma acceptance remains a separate evidence layer.

It also does not mean Semwright exposes private Figma APIs, app.asar patching, CDP/Yolo mode, arbitrary JavaScript, or unrestricted remote resource fetching. Those are outside the semantic/security contract by design.
