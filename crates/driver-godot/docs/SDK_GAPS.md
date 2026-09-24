# Godot findings against the current Driver SDK

The original isolated pack was based on Driver Protocol v1. Current Semwright main provides
Driver Protocol v2, so child events, cooperative cancellation, progress and artifact frames are
no longer gaps for this driver. The production Godot driver negotiates and exercises those
interfaces.

The remaining findings below should be solved generically, not by granting Godot ambient
authority.

## P0 — loopback-only network authority

Driver manifests still express network as a boolean. Godot needs only an authenticated local
editor bridge. The driver itself binds to `127.0.0.1`, but `network=true` gives the sandbox
broader network reach than an ideal loopback ACL. A future host grant should express and enforce
loopback-only connectivity.

## P0 — owner secret delivery

The host scrubs environment and has no first-class secret handle. Godot therefore reads
owner-generated pairing material from a private read-only config mount. This is bounded and
redacted, but a generic secret resource with rotation/revocation semantics would be stronger.
## P1 — broker session/authorization context

Driver Protocol v2 execution context carries request identity, cancellation and reporting
channels, not the broker caller/session authorization identity. Explicit Godot project/session
refs prevent accidental retargeting but do not create per-agent project grants inside one
driver instance.

## P1 — provider-owned opaque app refs

Godot refs are driver-local structured values with project/session/generation/revision/
fingerprint data. The broker does not yet materialize arbitrary provider-owned opaque native
references with generic stale-generation validation.

## P1 — persistent driver resource budgets

Linux `RLIMIT_CPU` is cumulative for a persistent process. Long-lived event-driven drivers may
need a distinction between lifetime limits and per-operation CPU budgets without weakening
memory, FD or process-tree confinement.

## P1 — secondary executable authority

The host pins/stages the driver executable, while Godot runner operations need a second
owner-approved executable. The driver currently validates Godot path and SHA-256 itself and
uses a fixed argument builder. A generic host-managed executable handle/staging primitive would
remove the remaining same-UID replacement race.
## P1 — companion plugin distribution

A Godot integration includes both a Rust driver and an `addons/semwright/` EditorPlugin tree.
Current driver distribution is centered on one executable payload. A reviewed multi-artifact
package format should install/update/remove companion application plugins without silently
activating them.

## P2 — outer dry-run context

Driver Protocol v2 does not carry the broker's outer dry-run bit in `DriverExecutionContext`.
Godot mutation schemas therefore include an explicit bounded `dry_run` field and still rely on
normal broker policy/confirmation. A future generic execution context could unify this semantic.

## Cross-platform confinement

Linux has a tested bubblewrap/Landlock path. macOS and Windows must retain their own fail-closed
host guarantees before Godot can be declared cross-platform through Semwright.

These are SDK/Host improvements, not reasons to expose arbitrary GDScript, shell execution,
unrestricted object calls or unsandboxed driver fallback.
