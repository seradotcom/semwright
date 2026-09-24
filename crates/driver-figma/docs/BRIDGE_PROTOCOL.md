# Bridge protocol v2

Transport is WebSocket bound only to `127.0.0.1`. No LAN/public bind is permitted.

Handshake:

```text
Hello(session/document/generation/plugin metadata)
  <- Challenge(nonce)
Authenticate(HMAC-SHA256 proof)
  <- Ready(revision)
Request / Response / Event / Ping-Pong / Close
```

A fresh 256-bit pairing secret is generated per driver lifetime. The proof binds the fresh server nonce, session ID and generation. Captured proofs cannot authenticate a new challenge; wrong proofs never register a session.

Requests carry correlation IDs, session/generation and optional expected revision. Responses from the wrong generation are stale. Duplicate/late IDs are rejected. Remote document-change events advance revision state.

Limits include a 1 MiB control-message ceiling, bounded pending requests, bounded session metadata and operation-specific semantic budgets. Export bytes are retained behind bounded plugin artifact tokens and retrieved with paginated `artifact.read` chunks rather than giant JSON arrays; successful export operations are promoted to Driver Protocol v2 `JobArtifact` metadata. A generic child-to-host binary stream/store handoff remains future SDK work.

The production Rust driver starts this bridge and `Driver::execute` dispatches bridge-backed operations through it. This bridge protocol is distinct from Semwright Driver Protocol v2: the driver negotiates `events=true` at the child-driver layer and forwards allowlisted plugin events as bounded `figma.*` child events. The production fake-Figma E2E exercises Driver Protocol v2 -> WebSocket -> fake-plugin request/response flow.

Security invariants: loopback only, ephemeral secret, HMAC authentication, strict serde envelopes, replay/generation rejection, no eval/Function/import surface, and untrusted Figma content treated as data.
