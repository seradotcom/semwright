# Generic Semwright SDK gaps exposed by the Figma driver

These are integration findings, not requests to weaken current sandboxing.

## G01 — Loopback-scoped network authority — GAP / high value

Driver Manifest v1 grants network as a boolean. The Figma bridge needs only an authenticated localhost WebSocket listener. An owner who enables this driver therefore grants broader network authority than the driver conceptually needs.

Desired generic direction: platform-enforced loopback/endpoint-scoped grants without changing Driver Protocol semantics.

## G02 — Child-driver events — GAP

The Figma plugin produces selection/page/document-change events and the Rust bridge consumes them for cache/revision invalidation. Driver Protocol v1 cannot publish those events into Semwright's Provider Event stream.

Current workaround: events remain internal to the driver and affect subsequent request/response semantics.

## G03 — Cooperative cancellation — GAP

Semwright core supports cancellation, but Driver Protocol v1 cannot send a cooperative cancel frame to a persistent child. Long exports would benefit from this.

Current semantic operations remain bounded request/response and no false cancellation guarantee is advertised.
## G04 — Dynamic capabilities — GAP

Actual availability differs by editor/session: Figma Design, FigJam, Motion Beta, fonts and document state. Driver Protocol v1 advertises a static catalog.

Current workaround: advertise the bounded cross-session semantic surface and fail unavailable operations explicitly at execution time.

## G05 — Binary artifacts / streams — GAP

PNG/PDF/animated exports should not travel as large JSON arrays. A generic child-driver artifact/file handoff with size/hash/lifetime metadata is preferable.

Until such a contract exists, large export capabilities should remain disabled or integrate with whatever artifact API exists in the target Semwright main.

## G06 — Persistent-process CPU accounting — GAP

The local bridge is long-lived while Linux Driver Host CPU seconds are cumulative RLIMIT-style accounting. A healthy persistent event-driven child can eventually exhaust a lifetime budget.

Do not remove resource limits; a renewable/windowed accounting model would be the generic solution.

## G07 — Job progress from child — GAP

Broker Jobs already exist, but Driver Protocol v1 does not negotiate child-originated progress/artifact lifecycle. Large Figma exports may eventually need this.
## G08 — Application-native refs — supported with limitation

Semwright's generic ref store does not directly encode Figma document/node identity. The driver therefore uses session generation + document identity + node ID/revision/fingerprint semantics.

This is workable, but real-Figma collaboration/reconnect acceptance is still required.

## Secret delivery note

The default PluginTransport does **not** need a persisted host-delivered secret: the driver generates an ephemeral per-run pairing secret and reveals it only through the explicit secret-access `pairing.begin` capability. Therefore generic secret delivery is not a blocker for core local Figma editing.

A future optional REST/OAuth transport would need Semwright secret references and must not put tokens in manifests/environment/logs.

No generic Driver Protocol or Semwright core contract change was required by this integration.
