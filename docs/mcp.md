# MCP frontend

`semwright-mcp` uses `rmcp` 3.4.0 and stdio. It contains no independent desktop execution
logic: all tool calls go through a broker Unix session. The official SDK owns negotiation,
framing and cancellation reception; the adapter propagates cancellation to broker work.
This source/API mapping has not been compiled or exercised by an MCP client here.

Always-visible tools are `computer_doctor`, `computer_capabilities`,
`computer_search_commands`, `computer_describe_command`, `computer_execute`,
`computer_snapshot`, `computer_find`, and `computer_audit_tail`. Discovery returns complete
command descriptors. The universal execution gateway validates again inside the broker.
Risk annotations are informational and do not substitute for authorization.

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
roots-based grant escalation or a tool for turning policy off. It does not implement
per-session dynamic registration of all application commands, a full task persistence API,
or MCP-driven privileged elicitation. Search/describe/execute is the deliberate small
surface. The read-only inspector and CLI use the same registry, errors and policy.
