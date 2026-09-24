# Motion Canvas rendering

Pinned renderer baseline: Motion Canvas **3.17.2**, Node **22.22.0**, Playwright **1.63.0**, Vite **5.4.21**.

Semwright render profiles use a half-open frame range `[first_frame, end_frame_exclusive)`. Motion Canvas 3.17.2 accepts an inclusive end time, so the pinned helper translates the final semantic frame to `(end_frame_exclusive - 1) / fps`; this prevents an extra exported frame at range boundaries.

No stable documented standalone Motion Canvas headless CLI was found for this baseline. Rendering therefore uses Motion Canvas' Vite project integration and core `Renderer` from a controlled browser harness. It is not pixel-click automation and does not record the Motion Canvas editor UI.

The Vite plugin is configured with the documented project import path `./src/project.ts` relative to the isolated build root. The disabled editor shim passes its absolute `main.js` module path because Motion Canvas 3.17.2 resolves the configured editor with Node module resolution and then loads `editor.html` and `styles.css` from that resolved module directory.

## Production path

1. Rust validates `semwright-motion.json` and a bounded `RenderProfile`.
2. Deterministic generated source is materialized in a content-addressed project tree.
3. Driver Host supplies an owner-approved read-only runtime mount with explicit `execute: true`, plus a separate non-executable read-only `fontconfig` system-config grant mapped only to `/etc/fonts`.
4. Rust verifies SHA-256 pins for Node, `render.mjs` and the exact Playwright Firefox executable.
5. A render job starts pinned Node only inside Driver Host. Node is capped with `--disable-wasm-trap-handler` and `--max-old-space-size=256`; the outer driver request remains at 128 tasks and 4 GiB virtual address space.
6. Rust pins `TMPDIR`, `TMP`, `TEMP` and XDG state to the job-specific writable output directory. The helper copies the generated project into that private area and performs the Vite build there.
7. After verifying the Driver Host marker, the helper launches only the pinned Firefox executable in headless mode with fixed sandbox-composition environment values. There is no request-controlled browser flag surface.
8. The helper creates a disposable Playwright context and intercepts the synthetic `semwright.invalid` origin from the local Vite build. All external page requests are aborted and no HTTP/CDP server is exposed.
9. Motion Canvas core `Renderer` is awaited directly. Its fixed Semwright exporter returns PNG data only through an owner-controlled Playwright binding.
10. Rust validates frame names/count, dimensions, PNG decode, pixel hash and alpha evidence before returning artifact paths.

If the browser-side render times out, the helper reports a bounded state snapshot (`phase`, last observed frame, renderer result/error) plus at most 32 bounded console/page-error diagnostics. This avoids turning a browser hang into an opaque timeout.

The helper never accepts arbitrary JavaScript, npm packages, commands or URLs from a driver request.

## Cancellation and timeout

Node is started in a new owned process group; Firefox descendants inherit that group. Cancellation or timeout terminates the group, escalates after a bounded grace period and deletes partial output. The Motion Canvas manifest currently negotiates protocol v1, so this driver does not transport protocol-v2 child progress events; status exposes observed phases only.

## Firefox sandbox layering

The certified Ubuntu path runs the Playwright-pinned Firefox build inside the already-required Driver Host Bubblewrap + Landlock boundary. The runtime executable is selected uniquely and SHA-256 pinned in the owner manifest. Firefox's nested content sandbox is disabled with fixed helper-owned environment settings only after the outer Semwright sandbox marker is present; this avoids nesting a second sandbox authority while keeping AppArmor, Landlock, explicit filesystem grants, process limits and `network=false` authoritative.

Chrome-for-Testing/Chromium was evaluated first. Multiple exact CI builds aborted with `SIGTRAP/int3` before a usable automation endpoint appeared, even after isolated AppArmor, task-budget and address-space experiments. The production route therefore fails away from that browser rather than weakening Semwright's sandbox.

Running `render.mjs` directly outside the Driver Host boundary fails closed.

## Launch film

Normal PR CI renders a short opaque sequence, a two-frame alpha sequence and exercises cancellation through the real Driver Host. It uploads only small evidence.

The manual `Motion Canvas launch film` workflow renders the complete 1,560-frame / 52-second 1920×1080 project on an ephemeral GitHub runner. A fixed FFmpeg argument vector converts the validated PNG sequence to a mezzanine because the current MLT driver imports bounded media files rather than image sequences. The real MLT provider then assembles video + deterministic WAV and performs the final H.264/AAC encode.

The final workflow artifact contains the MP4, poster, seven review frames, ffprobe evidence, SHA-256 sums and the real `DEMO_TRACE.json`. Full frame sequences and browser profiles are not persisted as repository content.
