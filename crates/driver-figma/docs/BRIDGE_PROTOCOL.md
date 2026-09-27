# Bridge protocol v2

Transport is WebSocket bound only to loopback. The bridge always binds `127.0.0.1` and also binds `::1` when IPv6 loopback is available, on the same port used by the plugin's `ws://localhost:<port>` URL. No LAN/public bind is permitted.

Handshake:

```text
Hello(session/document/generation/revision/plugin metadata)
  <- Challenge(nonce)
Authenticate(HMAC-SHA256 proof)
  <- Ready(revision)
Request / Response / Event / Ping-Pong / Close
```

A fresh 256-bit pairing secret is generated per driver lifetime. The proof binds the fresh server nonce, session ID and generation. Captured proofs cannot authenticate a new challenge; wrong proofs never register a session.

Requests carry correlation IDs, session/generation and optional expected revision. Responses from the wrong generation are stale. Duplicate/late IDs are rejected. Remote document-change events advance revision state.

Connection continuity is generation-based. After the first authenticated pairing, the plugin retains the pairing secret only in process memory. A transient WebSocket close triggers bounded exponential reconnect (250 ms to 4 s), reuses the same session ID and increments generation before performing a fresh server challenge/HMAC authentication. The server heartbeats idle sessions, retires stale generations on every exit path, clears pending waiters immediately when a connection dies or is superseded, and asks older generations to close when a newer generation authenticates. Manual Disconnect clears the in-memory secret and cancels reconnect attempts.

Bridge-backed requests remain bounded. If a session writer closes or a pending response channel disappears, that generation is invalidated immediately rather than remaining visible as a ghost session. A normal operation timeout does not tear down an otherwise healthy WebSocket: the timed-out request ID is retained in a bounded tombstone set so a late plugin response can advance revision state and be discarded without being misclassified as a duplicate protocol violation. Read/write callers therefore fail promptly on dead connections while slow-but-live sessions remain connected.

When `SEMWRIGHT_FIGMA_BRIDGE_LOG` is set, the bridge emits secret-safe JSON diagnostics to stderr for listener startup, authentication, supersession, disconnect cleanup, request completion/failure, operation name and latency. Pairing secrets, HMAC proofs and authorization material are never logged.

Limits include a 1 MiB control-message ceiling, bounded pending requests, bounded session metadata and operation-specific semantic budgets. Export bytes are retained behind bounded plugin artifact tokens and retrieved with paginated `artifact.read` chunks rather than giant JSON arrays; successful export operations are promoted to Driver Protocol v2 `JobArtifact` metadata. A generic child-to-host binary stream/store handoff remains future SDK work.

The production Rust driver starts this bridge and `Driver::execute` dispatches bridge-backed operations through it. This bridge protocol is distinct from Semwright Driver Protocol v2: the driver negotiates `events=true` at the child-driver layer and forwards allowlisted plugin events as bounded `figma.*` child events. The production fake-Figma E2E exercises Driver Protocol v2 -> WebSocket -> fake-plugin request/response flow.

Security invariants: loopback only, ephemeral secret, HMAC authentication, strict serde envelopes, replay/generation rejection, no eval/Function/import surface, and untrusted Figma content treated as data.
