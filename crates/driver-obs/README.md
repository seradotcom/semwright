# Semwright OBS realtime driver

This first-party driver exposes OBS Studio through a curated Semwright Driver SDK v1 surface over obs-websocket 5.x. It is not an OBS MCP server and does not use desktop GUI automation.

The driver provides 65 bounded capabilities covering status, scenes and scene items, inputs/audio, filters, transitions, recording, streaming, replay buffer, virtual camera, media, Studio Mode, bounded event polling and external-output operation inspection.

## Architecture

```text
Semwright broker / owner policy
  -> DriverProvider
  -> digest-pinned ELF
  -> Bubblewrap + Landlock
  -> semwright-obs-driver
  -> loopback obs-websocket
  -> OBS Studio
```

The production WebSocket actor owns request correlation, connection generations, bounded reconnects, cancellation of local waits, event backpressure and shutdown. Driver-local refs bind object identity to the observed OBS generation/graph revision. They are application handles, not broker authorization tickets.

Semwright Driver Protocol v1 cannot carry unsolicited child-driver events, cooperative cancellation or dynamic capabilities. The driver therefore advertises those interfaces as disabled. `events.poll` is bounded request/response polling, and `operations.get` describes external OBS output state; neither is presented as a native Semwright event/job.

## Security defaults

Configuration accepts literal loopback addresses only, defaults to 127.0.0.1:4455, rejects embedded passwords and disables `stream.start` unless explicitly configured. The manifest still requires `network=true`, and the owner must separately grant driver network authority. That current host grant is broader than loopback-only confinement.

OBS itself is outside the driver sandbox. Recording, media reads, browser sources and external publishing occur in the OBS process, not inside DriverProvider confinement.

## Verification

Run the core driver gates with:

```sh
python3 scripts/generate-obs-catalog.py --check
cargo clippy --locked -p semwright-driver-obs --all-targets -- -D warnings
cargo test --locked -p semwright-driver-obs --all-targets
```

The fake-OBS matrices exercise the production Rust client against an independent Python WebSocket fixture. `host_conformance.rs` additionally exercises the real DriverProvider, digest verification, Bubblewrap/Landlock and owner network opt-in when its explicit test environment variables are set.

Real OBS integration must always use a disposable profile/configuration. Tests must never attach to a normal user profile or start a real external stream.
