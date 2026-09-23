# MCP frontend

`semwright-mcp` uses the official `rmcp` 3.4.0 SDK over stdio. It contains no independent
desktop execution logic: all tool calls go through a broker Unix session. The SDK owns protocol
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

Replace paths/UID; do not paste illustrative values. The broker must already be running.
Both processes must share a login UID and runtime. For a private fake session use the fake
socket explicitly; never mistake a fixture response for observation of your desktop.

`--session-file` enables an explicitly shared private ticket; otherwise the server keeps
its session in-process. Tickets do not grant more capabilities and cannot approve sensitive
commands. Do not put tickets or browser debug endpoints in model prompts. Stdio stdout is
reserved for MCP; human diagnostics go to stderr.

The current implementation does not advertise arbitrary evaluation, approval, shell,
roots-based grant escalation or a tool for turning policy off. MCP Tasks are an adapter over
Semwright's bounded in-memory JobStore: task IDs remain broker-session scoped, are not durable
across daemon restarts, and never self-approve work. Semwright tasks currently never enter
`input_required`, so `tasks/update` rejects updates unless a future negotiated provider contract
adds an outstanding input request. Search/describe/execute remains the deliberate small surface.
The read-only inspector and CLI use the same registry, errors and policy.

The compatible catalog additions are `capabilities_search` and `capabilities_describe`. See [capability discovery](capabilities.md).
