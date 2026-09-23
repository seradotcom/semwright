# Motion Canvas rendering

Pinned renderer baseline: Motion Canvas **3.17.2**, Node **22.22.0**, Playwright **1.63.0**, Vite **5.4.21**.

Semwright render profiles use a half-open frame range `[first_frame, end_frame_exclusive)`. Motion Canvas 3.17.2 accepts an inclusive end time, so the pinned helper translates the final semantic frame to `(end_frame_exclusive - 1) / fps`; this prevents an extra exported frame at range boundaries.

No stable documented standalone Motion Canvas headless CLI was found for this baseline. Rendering therefore uses Motion Canvas' Vite project integration and core `Renderer` from a controlled browser harness. It is not pixel-click automation and does not record the Motion Canvas editor UI.

The Vite plugin is configured with the documented project import path `./src/project.ts` relative to the isolated build root; the disabled editor shim is supplied as a directory containing `editor.html`, `styles.css` and `main.js`, matching the upstream plugin contract.

## Production path

1. Rust validates `semwright-motion.json` and a bounded RenderProfile.
2. Deterministic generated source is materialized in a content-addressed project tree.
3. Driver Host supplies an owner-approved read-only runtime mount.
4. Rust verifies SHA-256 pins for Node, `render.mjs` and Chromium.
5. A render job starts the pinned Node helper only inside the Driver Host sandbox.
   Node is launched with `--disable-wasm-trap-handler` and `--max-old-space-size=256` so Vite/Undici remain compatible with the existing 4 GiB Driver Host address-space ceiling instead of raising that generic limit.
6. The helper copies the generated project to a private temporary directory and runs a Vite build with an absolute project entry.
7. A dedicated Playwright context loads the built output through intercepted requests at `semwright.invalid`; external requests are aborted and there is no listening HTTP socket.
8. Motion Canvas core `Renderer` invokes the fixed Semwright image-sequence exporter.
9. The exporter returns PNG data only through an owner-controlled Playwright binding.
10. Rust validates frame names/count, dimensions, PNG decode, pixel hash and alpha evidence before returning artifact paths.

The helper never accepts arbitrary JavaScript, npm packages, commands or URLs from a driver request.

## Cancellation and timeout

The Rust job owns a new process group. Cancellation or timeout terminates the group, escalates after a bounded grace period and deletes partial output. Driver Protocol v1 does not transport child progress events, so status exposes observed phases only.

## Chromium sandbox layering

GitHub Ubuntu 24.04 rejects Chromium's nested user-namespace sandbox. The helper therefore permits `chromiumSandbox:false` only when it inherits `SEMWRIGHT_DRIVER_SANDBOX=landlock-bwrap-v1`. That marker is created by the Driver Host path, not by an agent request. The outer Bubblewrap + Landlock sandbox remains active and the driver manifest has `network=false`.

Running `render.mjs` directly outside that boundary fails closed.

## Launch film

Normal PR CI renders a short opaque sequence, a two-frame alpha sequence and exercises cancellation through the real Driver Host. It uploads only small evidence.

The manual `Motion Canvas launch film` workflow renders the complete 1,560-frame / 52-second 1920×1080 project on an ephemeral GitHub runner. A fixed FFmpeg argument vector converts the validated PNG sequence to a mezzanine because the current MLT driver imports bounded media files rather than image sequences. The real MLT provider then assembles video + deterministic WAV and performs the final H.264/AAC encode.

The final workflow artifact contains the MP4, poster, seven review frames, ffprobe evidence, SHA-256 sums and the real `DEMO_TRACE.json`. Full frame sequences and browser profiles are not persisted as repository content.
