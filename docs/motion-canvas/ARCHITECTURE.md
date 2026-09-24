# Motion Canvas driver architecture

The first-party Motion Canvas integration is a Rust Driver Protocol v1 provider. The Rust process owns capability schemas, semantic validation, refs/revisions, dry-run/diff, atomic persistence, deterministic code generation, render-job lifecycle and artifact validation.

`semwright-motion.json` is the authoritative editable source. Generated TypeScript/TSX is a content-addressed derivative. The agent never receives an eval, arbitrary TypeScript, shell, package-install or remote-script capability.

```text
Agent
  -> Broker / policy / audit
  -> Driver Host (Bubblewrap + Landlock, network=false)
  -> semwright-motion-canvas-driver
  -> versioned managed motion model
  -> deterministic TS/TSX compiler
  -> owner-mounted, SHA-256-pinned Node + render helper + Firefox
  -> Motion Canvas 3.17.2 Renderer
  -> validated PNG sequence
  -> MLT provider for the launch-film final timeline/encode
```

## Process boundary

Motion Canvas does not provide a documented stable standalone headless render CLI in the pinned baseline. The integration therefore uses the supported Motion Canvas project/Vite/core-renderer surfaces through a narrow Node helper. The helper receives only project/output/config paths chosen by the Rust driver; it has no agent-defined source-code or command surface.

The render helper builds a temporary project copy and serves the resulting files to a dedicated Playwright page through request interception at the synthetic `semwright.invalid` origin. It does not open a Vite HTTP listener. Every other browser request is aborted.

On Linux, render execution fails closed unless `SEMWRIGHT_DRIVER_SANDBOX=landlock-bwrap-v1` was established by Driver Host. Firefox's nested content sandbox is disabled only inside that outer Bubblewrap + Landlock boundary because the inner tab-process sandbox cannot compose with the existing namespace. Driver Host network remains false.

## Project transaction

A mutation follows one path: load bounded source -> verify source SHA/revision-bound refs -> clone -> apply semantic operations -> validate -> semantic diff -> deterministic compile -> materialize content-addressed generated tree -> write temp semantic file -> fsync -> reparse/recompile -> recheck source fingerprint -> atomic rename -> fsync directory.

Dry-run stops before any write and returns the same prospective semantic diff/generated inventory used by a real commit.

## Jobs and artifacts

Driver Protocol v1 has no negotiated child events or cooperative cancellation. Rendering is therefore a driver-local job surface: `render.start/status/cancel/result`. Cancellation terminates the owned process group and removes partial output. Successful jobs validate exact frame names/count, PNG dimensions, decoded pixels, hashes and alpha evidence before returning path metadata.

The driver requests named `project`, `media`, `output` and `runtime` grants plus an explicit read-only `fontconfig` system-config grant mapped only to `/etc/fonts`. The runtime mount is owner-provided and read-only; Node, helper and Firefox are each SHA-256 pinned. Final binary media is never returned inside protocol JSON.
