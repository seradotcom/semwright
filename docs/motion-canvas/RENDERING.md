# Motion Canvas rendering

Pinned renderer baseline: Motion Canvas **3.17.2**, Node **22.22.0**, Playwright **1.61.1**, Vite **5.4.21**.

Semwright render profiles use a half-open frame range `[first_frame, end_frame_exclusive)`. Motion Canvas 3.17.2 accepts an inclusive end time, so the pinned helper translates the final semantic frame to `(end_frame_exclusive - 1) / fps`; this prevents an extra exported frame at range boundaries.

No stable documented standalone Motion Canvas headless CLI was found for this baseline. Rendering therefore uses Motion Canvas' Vite project integration and core `Renderer` from a controlled browser harness. It is not pixel-click automation and does not record the Motion Canvas editor UI.

The Vite plugin is configured with the documented project import path `./src/project.ts` relative to the isolated build root. The disabled editor shim passes its absolute `main.js` module path because Motion Canvas 3.17.2 resolves the configured editor with Node module resolution and then loads `editor.html` and `styles.css` from that resolved module directory.

The controlled build sets Motion Canvas `buildForEditor: true` only to select upstream `editorBootstrap()`. In v3.17.2 the production `bootstrap()` does not resolve the project-level plugin list, so without this switch the generated fixed `semwrightExporterPlugin` is absent from `ProjectMetadata` and `Renderer` returns `RendererResult.Error` before frame 0. No editor UI is launched: the harness still loads only its own render entry, the editor module remains a disabled shim, and agent input cannot supply plugins or JavaScript.

## Production path

1. Rust validates `semwright-motion.json` and a bounded `RenderProfile`.
2. Deterministic generated source is materialized in a content-addressed project tree.
3. Driver Host supplies an owner-approved read-only runtime mount with explicit `execute: true`, plus a separate non-executable read-only `fontconfig` system-config grant mapped only to `/etc/fonts`.
4. Rust verifies SHA-256 pins for Node, `render.mjs` and the exact Playwright Firefox executable.
5. A render job starts pinned Node only inside Driver Host. Node is capped with `--disable-wasm-trap-handler` and `--max-old-space-size=256`; the outer driver request uses the existing SDK maximum of 256 tasks and remains at 4 GiB virtual address space; this follows the measured Firefox 151 WebRender `EAGAIN` at 128 tasks.
6. Rust pins `TMPDIR`, `TMP`, `TEMP` and XDG state to the job-specific writable output directory. The helper copies the generated project into that private area and performs the Vite build there.
7. After verifying the Driver Host marker, the helper launches only the pinned Firefox executable in headless persistent-context mode with fixed sandbox-composition environment values and `dom.ipc.forkserver.enable=false`. It uses Playwright's startup `about:blank` page instead of requesting a second page after launch, because CI showed that the context-new-page operation could not create a target inside the nested namespace. The fixed fork-server preference separately bypasses Firefox's Linux fork-server broker after CI proved that broker could not create a tab subprocess inside the already-isolated Driver Host namespace; normal Firefox content processes remain enabled. There is no request-controlled browser flag surface.
8. The helper creates a disposable Playwright context and intercepts the synthetic `semwright.invalid` origin from the local Vite build. All external page requests are aborted and no HTTP/CDP server is exposed.
9. Motion Canvas core `Renderer` is awaited directly. Its fixed Semwright exporter returns PNG data only through an owner-controlled Playwright binding.
10. Rust validates every frame name, symlink boundary, compressed-byte SHA-256 and bounded PNG header/dimensions before returning artifact paths. Renders of up to 60 frames receive exhaustive pixel decode/hash/alpha evidence; longer sequences deeply validate five deterministic frame samples while preserving byte-level integrity evidence for every frame. That CPU/I/O work runs on Tokio's blocking pool so the current-thread Driver Protocol loop stays responsive.

If the browser-side render times out, the helper reports a bounded state snapshot (`phase`, last observed frame, renderer result/error) plus at most 32 bounded console/page-error diagnostics. This avoids turning a browser hang into an opaque timeout.

The helper never accepts arbitrary JavaScript, npm packages, commands or URLs from a driver request.

## Cancellation and timeout

Node is started in a new owned process group; Firefox descendants inherit that group. Cancellation or timeout terminates the group, escalates after a bounded grace period and deletes partial output. The Motion Canvas manifest currently negotiates protocol v1, so this driver does not transport protocol-v2 child progress events; status exposes observed phases only.

## Firefox version pin

The renderer deliberately pins Playwright 1.61.1 / Firefox 151.0. CI with Playwright 1.63.0 / Firefox 155.0 repeatedly reached Juggler and then failed to launch the tab subprocess with `SIGSEGV`; private shared memory did not change that outcome. The older exact pin is a compatibility response to a current upstream Firefox regression, not a relaxation of Semwright sandbox policy.

## Firefox sandbox layering

The certified Ubuntu path runs the Playwright-pinned Firefox build inside the already-required Driver Host Bubblewrap + Landlock boundary. The runtime executable is selected uniquely and SHA-256 pinned in the owner manifest. Firefox's nested content sandbox is disabled with fixed helper-owned environment settings only after the outer Semwright sandbox marker is present; this avoids nesting a second sandbox authority while keeping AppArmor, Landlock, explicit filesystem grants, process limits and `network=false` authoritative.

Chrome-for-Testing/Chromium was evaluated first. Multiple exact CI builds aborted with `SIGTRAP/int3` before a usable automation endpoint appeared, even after isolated AppArmor, task-budget and address-space experiments. The production route therefore fails away from that browser rather than weakening Semwright's sandbox.

Running `render.mjs` directly outside the Driver Host boundary fails closed.

## Launch film

Normal PR CI renders a short opaque sequence, a two-frame alpha sequence and exercises cancellation through the real Driver Host. It uploads only small evidence.

The manual `Motion Canvas launch film` workflow renders the complete 1,560-frame / 52-second 1920×1080 project on an ephemeral GitHub runner. A fixed FFmpeg argument vector converts the validated PNG sequence to a mezzanine because the current MLT driver imports bounded media files rather than image sequences. The real MLT provider then assembles video + deterministic WAV and performs the final H.264/AAC encode. The final MLT consumer remains offline/no-drop and keeps libx264 CRF 20 with the medium preset; it uses two MLT processing workers (`real_time=-2`), a five-frame buffer (`buffer=5`) and automatic codec thread selection (`threads=0`). This keeps offline/no-drop semantics while bounding buffering under the subprocess address-space limit.

The final workflow artifact contains the MP4, poster, seven review frames, ffprobe evidence, SHA-256 sums and the real `DEMO_TRACE.json`. Full frame sequences and browser profiles are not persisted as repository content.
