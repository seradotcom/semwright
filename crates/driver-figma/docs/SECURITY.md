# Figma driver security

## Trust boundaries

The Semwright broker/Driver Host owns policy, provenance and executable trust. The Rust Figma driver owns bridge authentication, schemas, refs/revisions and request correlation. The plugin main thread owns only official Figma Plugin API access; the UI iframe owns the loopback WebSocket.

No plugin message can grant Semwright authority.

## Certified transport

The production route is the official Plugin API bridge. The implementation does not patch `app.asar`, expose CDP, use arbitrary JavaScript evaluation, or bind the bridge to non-loopback interfaces.

Pairing uses an ephemeral 256-bit secret and HMAC-SHA256 challenge/response. Tests cover valid auth, invalid auth and captured-proof replay.

## Untrusted content

Layer names, text, SVG, variables, prototype metadata and collaborator-created content are untrusted data. Inputs and outputs are bounded. SVG import rejects script/foreignObject/event-handler/remote-resource patterns. Application text never becomes driver instructions.

## Collaboration

With dynamic-page access, the plugin loads pages once to enable `documentchange`. Only REMOTE change batches advance the collaboration revision through that listener; LOCAL writes are already revisioned by the mutation path. Real collaborative Figma acceptance is still required.

## Network and sandbox residual risk

The bridge itself needs loopback networking only, but Driver Manifest v1 exposes network as a boolean owner grant. The Linux sandbox therefore shares the host network namespace only after explicit owner opt-in, while the driver itself binds exclusively to 127.0.0.1. The narrower authority mismatch is documented in `SDK_GAPS.md`.

The repository includes a real DriverProvider host-conformance test. The local development machine cannot complete the mandatory bubblewrap user-namespace setup, so hosted native CI is the acceptance environment for that path. No unsandboxed fallback is used.

## Real-Figma boundary

No authorized disposable Figma session was available for this closeout. Fake-host and plugin-runtime evidence do not substitute for real Figma F1-F4 acceptance.
