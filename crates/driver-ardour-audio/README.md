# Ardour audio driver

First-party Ardour backend for `semwright-audio-domain`.

- Live control/observation: official Ardour OSC, fixed to loopback.
- Deep session inspection: owner-pinned Ardour Lua CLI plus a fixed Semwright Lua adapter.
- Application/native metadata is untrusted and bounded.
- Agent values never become Lua source, a host address, executable path or shell command.
- Projection fidelity and unsupported native semantics are explicit.

Deep/offline mutation primitives are kept behind the fixed adapter until each one has
snapshot-ref, stale-revision and differential-conformance evidence.
