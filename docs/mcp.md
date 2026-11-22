# MCP frontend

`semwright-mcp` uses the official `rmcp` 3.4.0 SDK over stdio. It contains no independent
desktop execution logic: all tool calls go through a local broker session. The SDK owns protocol
negotiation, framing, cancellation reception and the MCP Tasks wire contract; Semwright maps those
requests back to its session-scoped broker jobs.

The always-visible surface remains deliberately small: doctor/capability discovery, command
search/describe, snapshot/find/audit, `capabilities_search`, `capabilities_describe`, and two
execution gateways. `semwright_execute` is synchronous. `semwright_execute_task` is explicit
asynchronous execution and is usable only by clients that negotiated
`io.modelcontextprotocol/tasks`. Both validate again inside broker policy. Risk annotations are
informational and do not substitute for authorization.

```json
{"mcpServers":{"semwright":{"command":"/home/YOUR_USER/.local/bin/semwright-mcp","args":["--socket","/run/user/YOUR_UID/semwright/broker.sock"]}}}
```

Replace paths/UID; do not paste illustrative values. The broker must be running to
execute tools; discovery itself does not connect. On Unix both processes must share
a login UID and runtime. On Windows they must use the same authorized Windows user
and a local Semwright named pipe. For a private fake session use the fake socket
explicitly; never mistake a fixture response for observation of your desktop.

`--session-file` enables an explicitly shared private ticket; otherwise the server keeps
its session in-process. Tickets do not grant more capabilities and cannot approve sensitive
commands. The ticket path must be absolute. An explicitly selected broker `--socket` must
be an absolute Unix path, or on Windows a named pipe beginning with
`\\.\pipe\semwright-` (maximum 256 characters). Invalid connection settings fail
before MCP initialization and tool discovery, rather than advertising tools that can never
connect. A valid configuration does not require the broker to be running during discovery.

On Windows, the first-party MCP client additionally verifies the **actual opened** named-pipe
server, not merely a server advertised at the selected name. Before reading a saved
session ticket or sending the broker `Hello`, it checks the server's user SID,
logon session, pinned process instance, executable identity and SHA-256 from the
same registered Semwright portable bundle. The client's and broker's installed
executables and checksum/receipt files **and every directory ancestor up to the
volume root** must have trusted Windows ACLs that prevent replacement or
modification by other authenticated users. The filesystem-root exception allows
only normal new-sibling directory creation, not replacing or deleting installed
children. Authenticated Users, Users, or Everyone write, delete-child, or ACL
mutation permission on a package ancestor fails closed. Windows `OWNER RIGHTS`
(`S-1-3-4`) is accepted only for the directory's already verified
trusted owner, not as a grant to arbitrary authenticated users. A package with
inherited broad write permissions, a missing/altered receipt, or a foreign pipe
server fails closed; there is no alternate unverified connection. Install into an owner-protected
location and do not treat a pre-launch peer check as a replacement for this
connected-handle verification. A registered bundle is a local source-integrity
check, not a public code-signing or release certification.

Do not put tickets or browser debug endpoints in model prompts. Stdio stdout is
reserved for MCP; human diagnostics go to stderr.

MCP and the CLI preserve the local protocol's execution-outcome classification. If encoding
rejects an oversized request before sending any command bytes, its `InvalidArgument`
error has `outcome_known: true` and no operation was dispatched. A failed send,
lost or uncorrelated response, or cancellation after dispatch remains uncertain
(`outcome_known: false`). A cancellation while the broker handshake is still
pending returns a known `Cancelled` result promptly, before any `Execute` is sent;
it does not wait out the connection timeout or send an unmatched cancellation.
Neither classification triggers a retry or provider fallback. An uncertain
mutation requires reconciliation before another effect; cancellation does not
roll back work that may already have happened.

Revoking a broker session cancels its in-flight direct Core operations as well
as its managed jobs. Operations cancelled before provider dispatch return a
known `Cancelled`; after dispatch the result remains uncertain and cannot be
replayed automatically. Revocation does not cancel the transport's parent
token or another session's operations. A previously issued
`BrokerSessionExecutor` also retains a revocable session lease: once revoked,
it rejects subsequent descriptions and executions without dispatching another
provider call. Its per-session lease budget is bounded and fails closed with
`ResourceExhausted` when full; dropped executors release their lease.
The transport or connected host still owns admission of NEW executors and
requests and must reject an expired session ticket. Constructing a new
executor is not a substitute for authenticating a renewed session.

The current implementation does not advertise arbitrary evaluation, approval, shell,
roots-based grant escalation or a tool for turning policy off. MCP Tasks are an adapter over
Semwright's bounded in-memory JobStore: task IDs remain broker-session scoped, are not durable
across daemon restarts, and never self-approve work. Semwright tasks currently never enter
`input_required`, so `tasks/update` rejects updates unless a future negotiated provider contract
adds an outstanding input request. Search/describe/execute remains the deliberate small surface.
The read-only inspector and CLI use the same registry, errors and policy.

The compatible catalog additions are `capabilities_search` and `capabilities_describe`. See [capability discovery](capabilities.md).
