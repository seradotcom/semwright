# ADR 0001 — Source handoff boundaries and deviations

Status: accepted for this development snapshot, **not accepted as completed v1.0 scope**.
Date: 2026-09-21.

The requirements ask for a fully verified release candidate. The authoring environment
had no Rust/Cargo/cache and terminal downloads could not obtain a toolchain. Source and
available language/kernel/browser contract checks were therefore built, but no Rust pass,
lockfile or binary was fabricated. The version is 0.9.0-dev.1 and release admission fails
closed. This is an execution limitation and an incomplete outcome, not a reason to call
source complete or to restart the architecture.

Workspace boundaries are consolidated into 15 Rust crates plus one example plugin package.
Desktop backends share a crate and a narrow trait; application adapters share another.
MCP uses the official SDK and broker IPC, not a bespoke MCP implementation. GJS/KWin and
bpy source use the host language required by those applications.

Native zbus calls were used for portal and AT-SPI integration rather than adding ashpd/
atspi types across the core. This reduces schema coupling but shifts protocol conformance
responsibility to this implementation; it does not make untested APIs “verified”. EIS,
PipeWire pixels, persistent portal tokens and delta snapshots are explicitly unfinished.
No uinput/root fallback or unrestricted developer shell was included. These exclusions
must remain discoverable instead of silently escalating to xdotool/ydotool/shell.

For plugins, bubblewrap **and** Landlock enforcement are required, rather than falling back
to an unconfined child when a best-effort sandbox is unavailable. The daemon never elevates
or installs privileges. This strict choice reduces availability and needs kernel CI tests.

The recipe container is a typed version-1 object rather than the illustrative kind/apiVersion
wrapper. Redaction is conservative, not a formal noninterference theorem. Generic output
schemas and incomplete recipe progress paths are recorded blockers. The inspector is
read-only; MCP keeps a small universal discovery/execution surface instead of dynamic
hundreds-of-tools exposure.

The headless browser evidence uses an independent Python CDP probe and test-only HTML
injection. It is not substituted for a Rust adapter test. The Linux filesystem C harness
similarly probes kernel semantics, not compiled Rust policy. CI, packaging, benchmarks and
fuzz files are definitions, not executed artifacts. This distinction is maintained in
VERIFY.md and every compatibility row.
