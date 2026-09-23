# Semwright MLT video driver

This Linux-only persistent Driver SDK provider exposes 68 bounded capabilities for offline
video-project inspection, semantic timeline editing, safe save-as publication, render planning
and owned render jobs. It uses the normal `driver:mlt-video` policy scope and never accepts an
executable, shell command, environment or raw XML operation from capability arguments.

Deep mutation is supported only for the normalized MLT representation generated and validated
by this driver. Arbitrary Kdenlive and Shotcut documents are parsed conservatively and remain
read-only except for the explicitly documented metadata-risk surface. The original document is
never overwritten.

Timeline semantics are shared through `semwright-video-domain`, not defined by MLT XML. Before
a supported mutation is serialized, this driver projects the native project into the shared
model and runs the same edit through the backend-neutral engine. The projected MLT result,
created/affected identities and durations must match exactly or the operation fails closed.
Native XML bindings, service metadata and Kdenlive/Shotcut round-trip state remain private to
this driver.

Render planning follows the same boundary. Native encoder/runtime details such as `libx264`,
`melt`, service discovery, process supervision and output publication stay here, while the
driver projects each curated native render profile into the shared `RenderPreset` contract
(for example `libx264` becomes semantic `h264`). `render.plan` validates a shared
`RenderIntent` against the projected project before native preflight, so a future video backend
can share export intent without emulating MLT.

The production binary uses `semwright-driver-sdk`; `fake-melt` is compiled only with the
`test-tools` feature. A real render runtime additionally requires an owner-provided read-only
`runtime/runtime.json` containing exact SHA-256 pins for `melt`, `ffprobe` and `bwrap`.
Project/media roots are read-only and the output root is the only writable mount.

When launched by `DriverProvider`, the driver reuses the already-established Bubblewrap +
Landlock sandbox instead of attempting a nested user namespace, which Linux may reject after the
outer sandbox has dropped capabilities. Pinned media tools are still copied byte-for-byte into
private driver scratch, re-hashed, made non-writable, and supervised with explicit environment,
RLIMIT, process-group, timeout, cancellation and output budgets. Standalone driver execution keeps
the additional internal Bubblewrap layer; it is not the broker authorization boundary.

```sh
cargo test -p semwright-mlt-video-driver --all-features
cargo clippy -p semwright-mlt-video-driver --all-targets --all-features -- -D warnings
```

The repository test suite covers unit, property, differential-domain, round-trip, process,
protocol and security cases. Hosted native CI also launches the binary through the real Semwright
DriverProvider and sandbox. Those gates do not certify native round trips through real Kdenlive or
Shotcut, which remain explicit release evidence gaps.

## Shared video-domain contract

The driver now implements the shared backend projection contract in `src/domain.rs`.
Mutation preflight and format-level support reporting consume the resulting `BackendContract`;
the older adapter support methods remain the single native source of truth used to construct that
contract.

Native/application warnings no longer enter the portable semantic `Project`. They are emitted as
structured `ProjectionReport` losses, alongside read-only losses for opaque native assets,
tracks, effects, sequences and transitions. This keeps MLT round-trip metadata in the MLT layer
while preserving an adapter-neutral semantic core for future video backends.
