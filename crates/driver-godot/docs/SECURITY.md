# Godot driver security

The driver is a policy-mediated application provider, not an arbitrary Godot scripting gateway.

## Boundaries

- WebSocket bridge binds to loopback only and requires HMAC-SHA256 challenge/response pairing.
- Owner configuration is a private regular file and project/runner paths must be canonical.
- Capability descriptors and outputs are schema-validated; mutations support revision/fingerprint preconditions.
- Managed script writes are confined to canonical `res://` paths and reject `@tool`.
- The runner executes only a digest-pinned Godot binary with an allowlisted argument builder, scrubbed environment, private HOME, bounded logs, process-group kill, timeout and cooperative cancellation.
- Artifacts stay beneath the configured output root and are reported with hashes and sizes.
- Child events are bounded and namespaced as `godot.*`.

## Residual risks

Godot projects are executable software. `@tool` scripts, EditorPlugins, GDExtensions, importers and project scripts can execute in the Godot process. Typed commands do not make an untrusted project passive data.

Driver Manifest v1 still grants network as a boolean. The driver listens only on loopback, but the host grant is broader than an ideal loopback ACL.

`movie.capture` needs an owner-configured X11 display. A display is additional authority; the runner never discovers or inherits DISPLAY implicitly.
