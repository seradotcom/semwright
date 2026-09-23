# Generic Semwright SDK findings exposed by the Figma driver

These are integration findings against the current Driver Protocol v2 and Driver Host. They are not requests to weaken sandboxing.

## G01 — Loopback-scoped network authority — GAP / high value

The Driver Manifest still grants network as a boolean. The Figma bridge needs only an authenticated localhost WebSocket listener. An owner who enables this driver therefore grants broader network authority than the driver conceptually needs.

Desired generic direction: platform-enforced loopback/endpoint-scoped grants without changing the semantic driver API.

## G02 — Child-driver events — RESOLVED BY DRIVER PROTOCOL V2

Protocol v2 transports child events. The Figma driver negotiates `events=true`; selection, page and remote document-change notifications cross the authenticated bridge and are emitted as bounded `figma.*` driver events. Revision invalidation still happens inside the bridge before event forwarding.

## G03 — Cooperative cancellation — SDK SUPPORTED / FIGMA NOT ENABLED

Protocol v2 supports cooperative cancellation, but the current Figma surface is composed of bounded Plugin API operations that cannot guarantee remote rollback once a mutation has been dispatched. The driver therefore advertises `cooperative_cancellation=false` instead of making a false guarantee.

When a genuinely cancellable long-running Figma operation is introduced, it should use `DriverExecutionContext` and the protocol-v2 cancel frame.

## G04 — Dynamic capabilities — SDK SUPPORTED / FIGMA NOT ENABLED

Protocol v2 supports dynamic capability notifications. The Figma driver intentionally keeps a stable 91-capability catalog and performs editor/session/Motion availability checks at execution time. Operations without production handlers are not advertised.

## G05 — Binary artifacts / streams — SDK SUPPORTED, DRIVER DEFERRED

Protocol v2 supports artifact metadata alongside progress. The current Figma catalog does not advertise large binary export operations, so the driver negotiates `progress=false` and `artifacts=false`. Future PNG/PDF/animated export should use that generic artifact path rather than JSON byte arrays.

## G06 — Persistent-process CPU accounting — GAP

The local bridge is long-lived while Linux Driver Host CPU seconds are cumulative RLIMIT-style accounting. A healthy persistent event-driven child can eventually exhaust a lifetime budget.

Do not remove resource limits; a renewable/windowed accounting model would be the generic solution.

## G07 — Child progress — SDK SUPPORTED / FIGMA NOT ENABLED

Protocol v2 can report child-originated progress and artifacts. The current Figma operations are bounded request/response calls, so no progress interface is negotiated. A future long-running export path can adopt the existing protocol-v2 mechanism without another protocol change.

## G08 — Application-native refs — SUPPORTED WITH LIMITATION

Semwright's generic ref store does not directly encode Figma document/node identity. The driver therefore uses session generation + document identity + node ID/revision/fingerprint semantics.

This is workable, but real-Figma collaboration/reconnect acceptance is still required.

## Secret delivery note

The default PluginTransport does not need a persisted host-delivered secret: the driver generates an ephemeral per-run pairing secret and reveals it only through the explicit secret-access `pairing.begin` capability. A future optional REST/OAuth transport would need Semwright secret references and must not put tokens in manifests, environment variables or logs.
