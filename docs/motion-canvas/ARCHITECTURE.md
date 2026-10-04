# Motion Canvas driver architecture

The first-party Motion Canvas integration is a Rust driver negotiating Driver Protocol v7 for production rendering. The Rust process owns capability schemas, semantic validation, refs/revisions, dry-run/diff, atomic persistence, deterministic code generation, semantic render-job refs, progress reporting and artifact validation. Driver Host owns the executable runtime job, its session binding, cancellation, timeout and process-tree containment.

`semwright-motion.json` is the authoritative editable source. Generated TypeScript/TSX is a content-addressed derivative. For exact-version external projects, the same model may live in an isolated `.semwright/motion` managed island; Semwright publishes only a generated scene bridge and never rewrites human `src/project.ts`. The agent never receives an eval, arbitrary TypeScript, shell, package-install or remote-script capability.

```text
Agent
  -> Broker / policy / audit
  -> Driver Host (Bubblewrap + Landlock, network=false)
  -> semwright-motion-canvas-driver
  -> versioned managed motion model
  -> deterministic TS/TSX compiler
  -> SHA-256-pinned Host-managed Node tool
  -> embedded bounded render helper + owner-granted executable runtime bundle
  -> Playwright-pinned Firefox / Motion Canvas 3.17.2 Renderer
  -> validated PNG sequence
  -> MLT provider for the launch-film final timeline/encode
```

## Process boundary

Motion Canvas does not provide a documented stable standalone headless render CLI in the pinned baseline. The integration therefore uses the supported Motion Canvas project/Vite/core-renderer surfaces through a narrow Node helper embedded in the Rust binary and delivered over bounded stdin. The helper receives only Host-resolved project/output/fontconfig roots plus validated semantic relative names and fixed configuration; it has no agent-defined source-code, executable path or command surface.

The helper builds a temporary project copy and serves the built files to a dedicated Playwright Firefox page through request interception at the synthetic `semwright.invalid` origin. It does not open a Vite HTTP listener or browser-control socket. Every external page request is aborted.

Render ranges use an integer frame clock with a stable seconds representative for
the native renderer's ceiling operation. For a singleton range, the exporter
filters exactly one extra native tail frame after the requested frame has been
accepted by the awaited Host binding. Duplicate, out-of-order, noninteger and
other out-of-range frames still fail validation; multi-frame ranges retain the
ordinary exporter behavior.

Native failures retain an exclusive, private `native-failure-receipt.json` in
the selected output child, bound to the render input digest. The receipt is at
most 64 KiB and its local stack is at most 16 KiB. Public stdout and stderr carry
finite classifications and persistence status; raw stacks and browser details
stay in the local receipt. Failure to save a receipt cannot authorize an artifact.

On Linux, render execution fails closed unless `SEMWRIGHT_DRIVER_SANDBOX=landlock-bwrap-v1` was established by Driver Host. Node itself is an immutable SHA-pinned Host tool. The owner runtime bundle is read-only with explicit executable authority; the helper resolves Playwright's Firefox path and rejects it if its canonical target escapes that bundle. Bubblewrap + Landlock, explicit per-tool mounts, AppArmor and `network=false` remain authoritative. Node and its browser descendants are owned and reaped by the Host runtime-tool job rather than by a driver-created process group.

## Project transaction

A mutation follows one path: load bounded source -> verify source SHA/revision-bound refs -> clone -> apply semantic operations -> validate -> semantic diff -> deterministic compile -> materialize content-addressed generated tree -> write temp semantic file -> fsync -> reparse/recompile -> recheck source fingerprint -> atomic rename -> fsync directory.

Dry-run stops before any write and returns the same prospective semantic diff/generated inventory used by a real commit.

## Jobs and artifacts

The production driver negotiates Protocol v7. `render.start/status/cancel/result` expose a stable semantic job ref backed by one session-bound Host runtime-tool job, while `render.execute` waits on that same path with cooperative cancellation, observed-state progress and a validated artifact record. Parent-request cancellation is allowed to send only the narrow Host job cleanup cancel after the request token flips. Successful jobs validate exact frame names/count, every frame's bounded PNG header/dimensions and compressed-byte hash, plus exhaustive pixel evidence for short renders or deterministic deep pixel samples for long sequences before returning path metadata.

The driver requests named `project`, `media`, `output`, `runtime` and `fontconfig` grants. The `motion-node` tool is SHA-pinned separately and may receive only `project`, `output`, `runtime` and `fontconfig`. The runtime bundle is owner-provided, read-only and executable only by explicit manifest opt-in; the helper is embedded in Rust. The complete browser resource bundle is not yet an immutable Host-attested package, which remains a generic runtime-bundle gap. Final binary media is never returned inside protocol JSON.
