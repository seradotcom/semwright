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

Limits include a 1 MiB control-message ceiling, bounded pending requests, bounded session metadata and operation-specific semantic budgets. BinaryStart/Chunk/End remain reserved for a future artifact transport; large media exports must not be encoded as ordinary JSON arrays.

The production Rust driver starts this bridge and `Driver::execute` dispatches bridge-backed operations through it. This bridge protocol is distinct from Semwright Driver Protocol v2: the driver negotiates `events=true` at the child-driver layer and forwards allowlisted plugin events as bounded `figma.*` child events. The production fake-Figma E2E exercises Driver Protocol v2 -> WebSocket -> fake-plugin request/response flow.

Security invariants: loopback only, ephemeral secret, HMAC authentication, strict serde envelopes, replay/generation rejection, no eval/Function/import surface, and untrusted Figma content treated as data.
