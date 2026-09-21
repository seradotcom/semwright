# ADR 0001: provenance overlay and operation-specific discovery

Status: implemented and covered by Rust/SDK integration tests.

Keep the existing command descriptor wire schema and registry as the authority for CLI,
MCP, recipes and plugins. Add a separately typed provenance/search overlay instead of
breaking every v1 plugin and recipe descriptor. Cache compiled schemas at registration.
The broker projects operation availability from each provider's explicit operation probe
mapping. It must never infer a command is available because an unrelated probe succeeded.
Discovery is bounded and deterministic, and execution still enters central policy.

Rejected: a second MCP-only registry, thousands of static tools, embedding dependencies,
and trusting external tool annotations as authorization. Detailed per-object preconditions
remain provider responsibilities, rechecked immediately before side effects. A route probe
is not a live-desktop certification and not an authority grant.
