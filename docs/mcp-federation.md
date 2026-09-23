# MCP federation

Semwright can consume an external MCP server as a dynamic Provider while remaining the
authorization and audit boundary presented to the agent.

The initial implementation deliberately supports **owner-configured local stdio upstreams**.
Upstream definitions can be managed with local `semwright mcp upstream ...` commands, but
those commands are intentionally not broker/MCP capabilities and never grant authorization.
There is no agent-callable "install or trust an MCP server" operation.

## Execution path

```text
agent -> Semwright MCP/CLI -> broker -> policy/approval -> ExternalMcpProvider -> upstream MCP
```

The upstream never registers core authority. Its tools are imported under an owner-assigned
namespace such as `external.playwright.*`, and every descriptor is marked as untrusted
metadata with an independent Semwright digest.

Remote descriptions, annotations, schemas, tool errors and results are data. They cannot
change broker policy or approve their own calls.

## Conservative risk model

MCP `ToolAnnotations` are hints supplied by an external server. Semwright therefore does
not currently use them to downgrade risk. Imported tools are registered as
`privilege_sensitive`, non-idempotent operations with no dry-run contract.

Execution requires both:

1. an explicit owner grant for `external-mcp:<slug>`; and
2. foreground operator approval.

This is intentionally restrictive until Semwright has an owner-reviewed per-tool policy
override format.


## Owner-managed upstream registry

The preferred operator workflow stores upstream **definitions** in a separate owner-only file:

```text
$XDG_CONFIG_HOME/semwright/mcp-upstreams.toml
```

or `$HOME/.config/semwright/mcp-upstreams.toml` when `XDG_CONFIG_HOME` is unset. The parent
directory must be owner-controlled mode `0700`; an existing registry must be a regular,
single-link owner file with mode `0600`. Writes use a create-new temporary file, `fsync`,
atomic rename and parent-directory sync. Symlinks, duplicate slugs, unknown TOML fields and
oversized registries are rejected.

The registry contains **definitions only**. It cannot change `policy.allow`. For example,
adding `playwright` does not grant `external-mcp:playwright`; that scope must still be reviewed
and granted separately in daemon policy.

Typical lifecycle:

```sh
semwright mcp upstream add playwright /absolute/canonical/path/to/server \
  --arg=--stdio \
  --expected-name playwright \
  --expected-version 1.0.0

semwright mcp upstream list
semwright mcp upstream inspect playwright
semwright mcp upstream doctor playwright
semwright mcp upstream disable playwright
semwright mcp upstream enable playwright
semwright mcp upstream remove playwright
```

`add` computes the executable SHA-256 from the opened file unless `--sha256` is supplied.
`doctor` launches that exact pinned executable, negotiates MCP, obtains the tool catalog and
then shuts it down. `enable` revalidates the executable and digest; disabling/removing a stale
definition remains possible so an operator cannot be locked out by a changed binary.

Registry changes currently require a broker restart. The daemon can also be pointed at a
specific file with `--mcp-upstreams` or `SEMWRIGHT_MCP_UPSTREAMS`. The older inline
`[[trusted_mcp_stdio_upstreams]]` daemon configuration remains accepted for compatibility;
duplicate slugs across the two owner-controlled sources are rejected.

## Trusted stdio configuration

The daemon accepts an explicitly named configuration field:

```toml
[policy]
profile = "observe"
allow = ["external-mcp:playwright"]

[[trusted_mcp_stdio_upstreams]]
slug = "playwright"
program = "/absolute/canonical/path/to/mcp-server"
sha256 = "<lowercase sha256 of that executable>"
args = ["--stdio"]
expected_name = "playwright"
expected_version = "1.0.0"
request_timeout_ms = 30000
```

Generate the digest from the exact executable you intend to trust:

```sh
sha256sum /absolute/canonical/path/to/mcp-server
```

The daemon configuration itself must remain owner-controlled and mode `0600`. Inline
definitions do not receive precedence over the owner registry: duplicate slugs are rejected
rather than silently shadowed.

At startup Semwright verifies that the executable:

- is an absolute canonical path, not a symlink;
- is a bounded regular file;
- is not writable by group or others;
- matches the configured SHA-256;
- negotiates MCP successfully;
- matches an expected server name/version when those pins are configured.

The spawned child receives an empty environment, uses `/` as its working directory and
does not get a stderr channel into Semwright logs.

## Important trust boundary

**Digest pinning and Semwright policy do not sandbox the upstream process.**

A trusted stdio MCP executable still runs as the same Unix user and can potentially access
resources available to that UID. Only configure executables you independently trust.

Semwright mediates which imported tools the agent may call; it does not claim that this
first stdio launcher confines malicious upstream implementation code. A future sandboxed
upstream/driver launch mode must be separately implemented and negative-tested.

## Discovery

MCP `tools/list` is converted into ordinary Semwright capabilities. Names are normalized,
bounded and suffixed with a digest of the original upstream tool name to avoid ambiguous
collisions.

Use normal capability discovery rather than exposing every imported tool as a static MCP
tool:

```text
capabilities.search(provider="external-mcp:playwright", query="navigate")
capabilities.describe(name="external.playwright....")
execute(...)
```

The provenance returned with a capability identifies the external provider, provider
version, descriptor digest, catalog revision and execution generation.

## Dynamic tool lists

If an upstream sends `notifications/tools/list_changed`, Semwright refreshes that provider's
catalog transactionally.

A successful catalog refresh creates a new generation for future calls without cancelling a
call that was already dispatched with a pinned descriptor. Invalid replacement metadata
revokes that provider generation fail-closed. A terminal transport disconnect invalidates
the provider and prevents refresh from resurrecting that connection.

## Cancellation and failures

Semwright propagates command cancellation to an outstanding MCP request using the official
SDK cancellation mechanism. Broker timeouts also cancel the provider context.

External tool error text is not copied into trusted error channels. Semwright reports a
bounded generic backend failure and retains provenance/audit metadata.

## Current limits

The first federation pass intentionally does not claim:

- remote MCP transports;
- an agent-callable upstream installer or policy-grant operation;
- sandboxing of arbitrary stdio upstream executables;
- MCP input-required rounds;
- MCP task-result bridging into Semwright jobs;
- automatic trust of `ToolAnnotations`;
- persistence changes made by an upstream beyond its normal process behavior.

Those remain explicit follow-on work rather than hidden fallbacks.
