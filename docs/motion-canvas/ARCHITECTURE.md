# Motion Canvas driver architecture

The first-party Motion Canvas integration is a Rust driver negotiating Driver Protocol v3. The Rust process owns capability schemas, semantic validation, refs/revisions, dry-run/diff, atomic persistence, deterministic code generation, render-job lifecycle, cooperative cancellation, progress reporting and artifact validation.

`semwright-motion.json` is the authoritative editable source. Generated TypeScript/TSX is a content-addressed derivative. For exact-version external projects, the same model may live in an isolated `.semwright/motion` managed island; Semwright publishes only a generated scene bridge and never rewrites human `src/project.ts`. The agent never receives an eval, arbitrary TypeScript, shell, package-install or remote-script capability.

```text
Agent
  -> Broker / policy / audit
  -> Driver Host (Bubblewrap + Landlock, network=false)
  -> semwright-motion-canvas-driver
  -> versioned managed motion model
  -> deterministic TS/TSX compiler
  -> SHA-256-pinned Node + render helper + Firefox
  -> Motion Canvas 3.17.2 Renderer
  -> validated PNG sequence
  -> MLT provider for the launch-film final timeline/encode
```

## Process boundary

Motion Canvas does not provide a documented stable standalone headless render CLI in the pinned baseline. The integration therefore uses the supported Motion Canvas project/Vite/core-renderer surfaces through a narrow Node helper. The helper receives only project/output/config paths and the pinned browser path chosen by Rust; it has no agent-defined source-code or command surface.

The helper builds a temporary project copy and serves the built files to a dedicated Playwright Firefox page through request interception at the synthetic `semwright.invalid` origin. It does not open a Vite HTTP listener or browser-control socket. Every external page request is aborted.

On Linux, render execution fails closed unless `SEMWRIGHT_DRIVER_SANDBOX=landlock-bwrap-v1` was established by Driver Host. The owner runtime mount is read-only with explicit executable authority. After that marker is verified, the helper launches only the SHA-256-pinned Firefox executable with a fixed environment that disables Firefox's nested content sandbox; Bubblewrap + Landlock, explicit mounts, AppArmor and `network=false` remain the authority. Node and Firefox stay in the owned render process group for cancellation.

## Project transaction

A mutation follows one path: load bounded source -> verify source SHA/revision-bound refs -> clone -> apply semantic operations -> validate -> semantic diff -> deterministic compile -> materialize content-addressed generated tree -> write temp semantic file -> fsync -> reparse/recompile -> recheck source fingerprint -> atomic rename -> fsync directory.

Dry-run stops before any write and returns the same prospective semantic diff/generated inventory used by a real commit.

## Jobs and artifacts

The driver negotiates Protocol v3. The legacy asynchronous job surface (`render.start/status/cancel/result`) remains available, while `render.execute` maps the same renderer and job registry onto one protocol-owned request lifecycle with cooperative cancellation, observed-state progress messages and a validated artifact record. Cancellation terminates the owned process group and removes partial output. Successful jobs validate exact frame names/count, every frame's bounded PNG header/dimensions and compressed-byte hash, plus exhaustive pixel evidence for short renders or deterministic deep pixel samples for long sequences before returning path metadata.

The driver requests named `project`, `media`, `output` and `runtime` grants plus an explicit read-only `fontconfig` system-config grant mapped only to `/etc/fonts`. The runtime mount is owner-provided, read-only and executable only by explicit Driver manifest opt-in; Node, helper and Firefox are SHA-256 pinned. Final binary media is never returned inside protocol JSON.
