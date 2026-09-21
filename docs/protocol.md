# Local broker protocol v1

Transport: a user-owned Unix stream socket, mode `0600`, in a user-owned private runtime
directory. Both client and server validate the peer UID. TCP is not a broker transport.
Each message is UTF-8 JSON prefixed by a **four-byte unsigned big-endian length**. Empty
payloads and payloads over 1,048,576 bytes are rejected before buffer allocation.

First client message:

```json
{"type":"hello","version":1,"session":null}
```

Server response:

```json
{"type":"welcome","version":1,"session":"<opaque UUID ticket>"}
```

Subsequent request:

```json
{"type":"execute","id":"<fresh 32-hex request ID>","request":{"command":"doctor","args":{},"dry_run":false}}
```

Responses use `{"type":"result","envelope":{...}}`. Other client messages are
`cancel` with an `id`, `subscribe` with an `after` sequence, and `ping`. Server messages
include `event`, `pong` and a structured `error`. The checked-in JSON Schema documents
shape; Rust parsing/server state still supply bounds, authorization and lifecycle rules.

The current server caps 64 connections, 32 sessions, 16 in-flight requests per session,
eight per connection and 4,096 remembered request IDs per session. Hello has a five-second
deadline, idle reads 300 seconds, writes ten seconds; session idle expiration is 1,800
seconds. Cancellation/connection loss invalidates the request's child token. It cannot
prove an already-dispatched application effect was rolled back.

A resumption ticket is private capability material, not an approval token. Client storage
uses `0600`, no-follow regular files and a lock around handshake/ticket replacement.
Refs expire after 60 seconds and remain bound to the issuing session/backend. Restarting
or resuming an expired session must not reinterpret old references.

Events contain command names, request IDs, backend/result metadata, not UI/document text.
History is limited to 256 entries; an expired cursor returns `Conflict`. A client should
request a fresh snapshot, not infer continuity from a gap. The implemented event stream
is predominantly broker-command/registry metadata, not the full desktop observation event
API envisioned in the brief.

Exit codes: 0 success; 1 other failure; 2 invalid input/protocol; 3 permission/policy/consent;
4 missing/stale/ambiguous target; 5 unsupported/unavailable/sandbox denied; 6 timeout/cancel.
Check the machine-readable error, especially `outcome_known`, before deciding to retry.
