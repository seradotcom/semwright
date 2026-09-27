# Godot driver security

The driver is a policy-mediated application provider, not an arbitrary Godot scripting gateway.

## Boundaries

- Broker/Policy/Audit remains the authorization authority. The Godot driver does not implement a
  second caller-authorization policy.
- Under Driver Host the editor bridge is exposed through a Host-owned `127.0.0.1` listener
  proxied to a private Unix socket inside the sandbox. The driver does not need ambient network
  authority.
- The bridge requires bounded HMAC-SHA256 challenge/response pairing before a session is usable.
- Production pairing material is delivered through first-class read-only, non-executable secret
  mounts beneath `/run/secrets/<name>`. Inline secrets require explicit development mode.
- Capability descriptors and outputs are schema-validated; mutations support
  revision/fingerprint preconditions.
- Provider-owned Godot refs are materialized by the Broker RefStore as opaque IDs and are
  validated by the provider before reuse.
- Managed script writes are confined to canonical `res://` paths and reject `@tool`.
- The primary driver executable is digest-verified and staged by Driver Host.
- Secondary Godot runner executables are digest-pinned, verified from a stable file descriptor
  and staged as sealed executable payloads. Runner arguments are built from an allowlist; no
  shell is involved.
- Linux confinement uses bubblewrap plus Landlock and has no unsandboxed fallback.
- Persistent drivers retain hard lifetime resource limits; optional per-operation CPU accounting
  bounds CPU consumed by the provider process tree.
- Artifacts stay beneath configured output roots and are reported with hashes and sizes.
- Child events are bounded and namespaced as `godot.*`.
- Driver Package v2 companion files use explicit relative destinations, per-file digests and
  size budgets. Installation does not activate the EditorPlugin inside a user project.

## Project trust boundary

Godot projects are executable software. `@tool` scripts, EditorPlugins, GDExtensions, custom
importers and project scripts can execute in the Godot process. Typed semantic commands do not
turn an untrusted project into passive data.

Only owner-approved project roots should be opened through the production driver. Disposable
fixtures should be used for hostile or unknown projects.

## Explicit additional authority

`movie.capture` requires an owner-configured X11 display. A display is additional authority;
the runner never discovers or inherits `DISPLAY` implicitly.

Godot export operations may require owner-installed export templates. Package v2 can distribute
the reviewed Semwright EditorPlugin files, but enabling those files inside a project remains an
explicit owner action.

## Platform scope

The certified security path for this closeout is Linux x86_64. A successful source build on
another operating system is not treated as evidence of equivalent Driver Host confinement or
real Godot acceptance.

## Session continuity

Transient editor-bridge loss uses authenticated logical-session resume. The plugin retains the session ID only in process memory and presents it as `resume_session` on reconnect. The driver preserves the logical session only for a short reconnect grace, authenticates a fresh HMAC transcript, creates a new generation, and prevents cleanup from an old connection from removing the new generation.

The companion uses bounded exponential reconnect and answers application-level heartbeat pings. Protocol/authentication failures fail closed instead of becoming infinite reconnect loops.
