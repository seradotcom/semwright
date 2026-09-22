# Semwright MLT video driver

This Linux-only persistent Driver SDK provider exposes 68 bounded capabilities for offline
video-project inspection, semantic timeline editing, safe save-as publication, render planning
and owned render jobs. It uses the normal `driver:mlt-video` policy scope and never accepts an
executable, shell command, environment or raw XML operation from capability arguments.

Deep mutation is supported only for the normalized MLT representation generated and validated
by this driver. Arbitrary Kdenlive and Shotcut documents are parsed conservatively and remain
read-only except for the explicitly documented metadata-risk surface. The original document is
never overwritten.

The production binary uses `semwright-driver-sdk`; `fake-melt` is compiled only with the
`test-tools` feature. A real render runtime additionally requires an owner-provided read-only
`runtime/runtime.json` containing exact SHA-256 pins for `melt`, `ffprobe` and `bwrap`.
Project/media roots are read-only and the output root is the only writable mount.

```sh
cargo test -p semwright-mlt-video-driver --all-features
cargo clippy -p semwright-mlt-video-driver --all-targets --all-features -- -D warnings
```

The repository test suite covers 206 unit, property, round-trip, process, protocol and security
cases. Hosted native CI also launches the binary through the real Semwright DriverProvider and
sandbox. Those gates do not certify native round trips through real Kdenlive or Shotcut, which
remain explicit release evidence gaps.
