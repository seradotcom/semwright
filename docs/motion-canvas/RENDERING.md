# Motion Canvas rendering

Pinned renderer baseline: Motion Canvas **3.17.2**, Node **22.22.0**, Playwright **1.63.0**, Vite **5.4.21**.

Semwright render profiles use a half-open frame range `[first_frame, end_frame_exclusive)`. Motion Canvas 3.17.2 accepts an inclusive end time, so the pinned helper translates the final semantic frame to `(end_frame_exclusive - 1) / fps`; this prevents an extra exported frame at range boundaries.

No stable documented standalone Motion Canvas headless CLI was found for this baseline. Rendering therefore uses Motion Canvas' Vite project integration and core `Renderer` from a controlled browser harness. It is not pixel-click automation and does not record the Motion Canvas editor UI.

The Vite plugin is configured with the documented project import path `./src/project.ts` relative to the isolated build root. The disabled editor shim passes its absolute `main.js` module path because Motion Canvas 3.17.2 resolves the configured editor with Node module resolution and then loads `editor.html` and `styles.css` from that resolved module directory.

## Production path

1. Rust validates `semwright-motion.json` and a bounded RenderProfile.
2. Deterministic generated source is materialized in a content-addressed project tree.
3. Driver Host supplies an owner-approved read-only runtime mount plus a separate read-only `fontconfig` system-config grant mapped only to `/etc/fonts`.
4. Rust verifies SHA-256 pins for Node, `render.mjs` and the exact Playwright Chromium headless shell executable.
5. A render job starts the pinned Node helper only inside the Driver Host sandbox. Node is launched with `--disable-wasm-trap-handler` and `--max-old-space-size=256` to bound its own heap. The Motion Canvas manifest separately requests the 16 GiB Driver Host virtual-address-space ceiling because real Chromium headless shell could not launch under the former 4 GiB ceiling; this is an `RLIMIT_AS` reservation limit, not a resident-memory allocation.
6. The parent pins `TMPDIR`, `TMP`, `TEMP` and XDG state to the job-specific writable output directory before the helper starts. The helper copies the generated project into that private area and performs the Vite build there.
7. A dedicated Playwright Chromium headless shell context loads the built output through intercepted requests at `semwright.invalid`; external requests are aborted and there is no listening HTTP socket.
8. Motion Canvas core `Renderer` invokes the fixed Semwright image-sequence exporter.
9. The exporter returns PNG data only through an owner-controlled Playwright binding.
10. Rust validates frame names/count, dimensions, PNG decode, pixel hash and alpha evidence before returning artifact paths.

The helper never accepts arbitrary JavaScript, npm packages, commands or URLs from a driver request.

## Cancellation and timeout

The Rust job owns a new process group. Cancellation or timeout terminates the group, escalates after a bounded grace period and deletes partial output. The Motion Canvas manifest currently negotiates protocol v1, so this driver does not transport protocol-v2 child progress events; status exposes observed phases only.

## Chromium headless shell sandbox layering

The certified Ubuntu path keeps Chromium headless shell inside the already-required Driver Host Bubblewrap + Landlock boundary and does not expose a request-controlled browser sandbox override. The exact Playwright-installed Chromium headless shell executable is selected uniquely and SHA-256 pinned in the owner runtime manifest. After verifying the Driver Host marker, the helper uses Playwright's `chromiumSandbox:false` together with fixed `--single-process --no-zygote --disable-gpu` arguments. This avoids a second namespace/sandbox and renderer-process tree inside the Driver Host boundary. The renderer admits only one generated local origin and aborts every external request; the browser remains confined by Bubblewrap/Landlock, the explicit filesystem grants and `network=false`. Playwright's temporary profile is created under the job's writable output root rather than depending on ambient host state.

Running `render.mjs` directly outside the Driver Host boundary fails closed.

## Launch film

Normal PR CI renders a short opaque sequence, a two-frame alpha sequence and exercises cancellation through the real Driver Host. It uploads only small evidence.

The manual `Motion Canvas launch film` workflow renders the complete 1,560-frame / 52-second 1920×1080 project on an ephemeral GitHub runner. A fixed FFmpeg argument vector converts the validated PNG sequence to a mezzanine because the current MLT driver imports bounded media files rather than image sequences. The real MLT provider then assembles video + deterministic WAV and performs the final H.264/AAC encode.

The final workflow artifact contains the MP4, poster, seven review frames, ffprobe evidence, SHA-256 sums and the real `DEMO_TRACE.json`. Full frame sequences and browser profiles are not persisted as repository content.
