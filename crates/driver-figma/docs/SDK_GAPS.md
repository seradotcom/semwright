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

Protocol v2 supports dynamic capability notifications. The Figma driver intentionally keeps a stable typed semantic catalog generated from the pinned public API baselines and performs editor/session/Motion/plan availability checks at execution time. Catalog consistency and semantic-completeness gates prevent advertising operations without handlers or classified transport semantics.

## G05 — Binary artifacts / streams — DRIVER IMPLEMENTED WITH LOCAL ARTIFACT TOKENS / GENERIC PROMOTION OPEN

Figma static and animated exports are advertised and avoid giant JSON byte arrays: the plugin stores bounded binary artifacts and exposes tokenized chunk reads/releases over the authenticated bridge. Protocol v2 also supports generic artifact metadata, but the Figma child does not yet negotiate `artifacts=true` or promote plugin artifact tokens into broker-native artifacts automatically.

A future generic promotion path can remove the driver-local read/release lifecycle without changing the semantic export operations.

## G06 — Persistent-process CPU accounting — GAP

The local bridge is long-lived while Linux Driver Host CPU seconds are cumulative RLIMIT-style accounting. A healthy persistent event-driven child can eventually exhaust a lifetime budget.

Do not remove resource limits; a renewable/windowed accounting model would be the generic solution.

## G07 — Child progress — SDK SUPPORTED / FIGMA NOT ENABLED

Protocol v2 can report child-originated progress and artifacts. The current Figma operations are bounded request/response calls, so no progress interface is negotiated. A future long-running export path can adopt the existing protocol-v2 mechanism without another protocol change.

## G08 — Application-native refs — SUPPORTED WITH LIMITATION

Semwright's generic ref store does not directly encode Figma document/node identity. The driver therefore uses session generation + document identity + node ID/revision/fingerprint semantics.

This is workable, but real-Figma collaboration/reconnect acceptance is still required.

## Secret delivery note

The default PluginTransport does not need a persisted host-delivered secret: the driver generates an ephemeral per-run pairing secret and reveals it only through the explicit secret-access `pairing.begin` capability.

The REST transport is now implemented and reads credentials only from an owner-provisioned same-UID protected Unix credential socket. Tokens are not capability arguments, manifests, environment variables, outputs or logs. A generic Semwright SecretRef/credential-helper abstraction would still be preferable to the current per-driver socket convention.
