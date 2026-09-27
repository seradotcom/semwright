# Driver session continuity

Semwright treats a driver process and the application's own transport as separate failure
boundaries. A healthy Driver Host child may temporarily lose its editor, WebSocket, Unix socket
or other provider connection without becoming a zombie provider or blocking broker requests
indefinitely.

The portable primitives live in `semwright_driver_sdk::continuity`. They intentionally do not
prescribe WebSocket, process, IPC or application authentication details.

## Required invariants

A persistent first-party driver must preserve the following invariants whenever its application
transport can disappear independently of the Driver Host process:

1. **Bounded requests.** Every request has a deadline. A dead transport cannot leave a request
   waiter alive indefinitely.
2. **Disconnect wake-up.** Transport loss closes or fails every waiter owned by that transport
   generation. The Driver Host also clears pending Protocol-v2 waiters if the driver child exits.
3. **No ghost session.** Health/session listings describe active application connections
   truthfully. A connection in reconnect grace is not reported as an active session.
4. **Generation freshness.** Every re-established transport gets a newer logical generation or a
   fresh provider-specific generation token. Responses, events and refs from an older generation
   are rejected or ignored.
5. **Bounded recovery.** Automatic retry uses a bounded reconnect policy/backoff and eventually
   becomes unavailable rather than spinning forever.
6. **Mutation outcome discipline.** If transport loss makes a mutation result unknowable, the
   error remains outcome-unknown. Semwright must not automatically replay a non-idempotent
   mutation merely because reconnect succeeded.
7. **Independent health.** Driver health remains queryable while the external application is
   absent whenever the driver process itself is healthy.
8. **Cleanup by generation.** Cleanup from an older socket/process generation cannot remove a
   newer replacement session.
9. **Explicit shutdown.** Driver shutdown cancels reconnect work and drains/invalidates
   transport-owned state.
10. **Observable lifecycle.** Drivers expose enough bounded state for conformance to distinguish
    connected, reconnecting and unavailable behavior without logging credentials.

`ConnectionState`, `ContinuityStamp`, `ReconnectPolicy`, `ReconnectBudget`, `RequestIds` and
`ContinuitySnapshot` provide the shared vocabulary. Drivers may use stronger application-native
identities/revisions in addition to these primitives.

## Authentication and trusted resume

Authentication is transport-specific. A reconnect feature must never turn a one-time pairing
secret into an unbounded permanent password.

Figma uses an authenticated localhost WebSocket. After a successful manual pairing the driver
issues a separate, opaque resume credential. The plugin stores only that credential in
Figma `clientStorage`, scoped to the current document. Each resume uses a fresh nonce/HMAC and a
new generation while refreshing the credential's bounded server-side TTL. Keeping the same opaque
credential until explicit revocation avoids a failure window where the application could restart
after authentication but before persisting a rotated replacement. Manual Disconnect revokes the
current credential. A driver restart clears the server-side credential table, so the stored
credential fails closed and the user must pair again.

Godot's owner configuration already supplies project pairing material independently of the
EditorPlugin process. Short transport reconnects can resume the same logical session with a fresh
generation; a full editor restart authenticates a fresh session automatically from the same
owner-provided secret. No interactive pairing is needed.

OBS already implements bounded reconnect generations, stale-generation rejection, pending
request cleanup and unknown mutation outcomes in its production actor. Its regression suite is
checked against the common SDK continuity semantics rather than replacing that mature actor.

Process-backed drivers such as MLT or LibreOffice do not need WebSocket heartbeat/pairing. They
still inherit the bounded Driver Host request/child-loss contract and must make external process
loss observable and bounded.

## Conformance

The `Driver session continuity` GitHub Actions workflow exercises the common contract and
fault-injection paths without using the developer workstation as a heavy runner:

- SDK state/backoff/generation contract;
- Driver Host child transport loss and pending waiter wake-up;
- process-backed stdio driver regression for Blender, LibreOffice, MLT Video and Motion Canvas;
- Figma forced disconnect, generation recovery and trusted-resume replay protection/revocation;
- Figma plugin storage/resume tests and production asset build;
- Godot short reconnect across a waiting request plus the normal pinned real-Godot acceptance;
- OBS generation recovery, reconnect-storm bounds, unknown mutation outcome and mid-concurrency
  disconnect cleanup.

Application-specific integration workflows remain the source of real-application evidence. The
common contract does not convert fake fixtures into proof of real editor interoperability.
