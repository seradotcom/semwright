# Motion Canvas rendering

Pinned renderer baseline: Motion Canvas **3.17.2**, Node **22.22.0**, Playwright **1.61.1**, Vite **5.4.21**.

Semwright render profiles use a half-open frame range `[first_frame, end_frame_exclusive)`. Motion Canvas 3.17.2 accepts an inclusive end time, so the pinned helper translates the final semantic frame to `(end_frame_exclusive - 1) / fps`; this prevents an extra exported frame at range boundaries.

No stable documented standalone Motion Canvas headless CLI was found for this baseline. Rendering therefore uses Motion Canvas' Vite project integration and core `Renderer` from a controlled browser harness. It is not pixel-click automation and does not record the Motion Canvas editor UI.

The Vite plugin is configured with the documented project import path `./src/project.ts` relative to the isolated build root. The disabled editor shim passes its absolute `main.js` module path because Motion Canvas 3.17.2 resolves the configured editor with Node module resolution and then loads `editor.html` and `styles.css` from that resolved module directory.

The controlled build sets Motion Canvas `buildForEditor: true` only to select upstream `editorBootstrap()`. In v3.17.2 the production `bootstrap()` does not resolve the project-level plugin list, so without this switch the generated fixed `semwrightExporterPlugin` is absent from `ProjectMetadata` and `Renderer` returns `RendererResult.Error` before frame 0. No editor UI is launched: the harness still loads only its own render entry, the editor module remains a disabled shim, and agent input cannot supply plugins or JavaScript.

## Production path

The pinned native adapter corrects SVG rounded rectangles before rendering: an omitted `rx` or `ry` inherits its partner, radii clamp to half the rectangle dimensions, and an explicit zero produces square corners. Circular radii apply to all four corners; elliptical corners use a native path with four elliptical arcs, preserving the extracted transform and style. This corrects Motion Canvas 3.17.2's interpretation of SVG `[rx, ry]` as alternating corner radii. Source SVG assets remain unchanged.

1. Rust validates `semwright-motion.json` and a bounded `RenderProfile`.
2. Deterministic generated source is materialized in a content-addressed project tree under the owner-granted `project` root.
3. Driver Host supplies a SHA-pinned `motion-node` tool plus the exact per-tool `project`, `output`, read-only executable `runtime`, and read-only `fontconfig` grants. No installation path is discovered by the driver.
4. Rust embeds `render.mjs` at compile time and submits it as bounded stdin to Node. Project/output/fontconfig paths travel as protocol-v7 typed Host refs; only validated semantic relative names remain ordinary literals.
5. Driver Host creates a detached session-bound Node job with the runtime root as its logical cwd. Node is capped with `--disable-wasm-trap-handler` and `--max-old-space-size=256`; process/time/resource containment belongs to the Host job rather than a private driver launcher.
6. The helper canonicalizes every Host-resolved root, combines it only with validated relative project/output names, and rejects any escape. Firefox writable profile/cache/tmp state is redirected to the job-specific output directory.
7. The helper resolves Playwright's Firefox executable from the runtime bundle and rejects it unless its canonical path remains inside that bundle. After verifying the Driver Host marker it launches Firefox in headless persistent-context mode with fixed sandbox-composition environment values and `dom.ipc.forkserver.enable=false`. There is no request-controlled browser flag or executable-path surface.
8. The helper creates a disposable Playwright context and intercepts the synthetic `semwright.invalid` origin from the local Vite build. All external page requests are aborted and no HTTP/CDP server is exposed.
9. Before the first render, both declarative and authoring routes demand-load the CSS-pinned Instrument Sans weights 400/500/600/700 and IBM Plex Mono400. A bounded browser gate checks nonempty loaded faces, family/style/weight readbacks, `FontFaceSet.ready`, and availability before and after readiness. Missing faces or a deadline fail the render. Each successful native job writes a private, exclusive `font-readiness-receipt.json` bound to the render input, font resource digest, frame range and rational frame rate; it records browser readiness, not reference bitmap identity. Motion Canvas core `Renderer` is then awaited directly. Its fixed Semwright exporter returns PNG data only through an owner-controlled Playwright binding.
10. After the Host job reports success, Rust validates every frame name, symlink boundary, compressed-byte SHA-256 and bounded PNG header/dimensions before returning artifact paths. Renders of up to 60 frames receive exhaustive pixel decode/hash/alpha evidence; longer sequences deeply validate five deterministic frame samples while preserving byte-level integrity evidence for every frame. That CPU/I/O work runs on Tokio's blocking pool so the Driver Protocol loop stays responsive.

If the browser-side render times out, the helper reports a bounded state snapshot (`phase`, last observed frame, renderer result/error) plus at most 32 bounded console/page-error diagnostics. This avoids turning a browser hang into an opaque timeout.

The helper never accepts arbitrary JavaScript, npm packages, commands or URLs from a driver request.

## Cancellation and timeout

Node is started as a session-bound Host runtime-tool job and Firefox descendants remain inside that Host-owned process boundary. Timeout, explicit `render.cancel`, provider shutdown and `render.execute` request cancellation all converge on Host job cancellation/reaping. Protocol v7 reports observed render-state transitions plus the validated terminal artifact through the protocol context. The asynchronous `render.start/status/cancel/result` surface and synchronous `render.execute` share the same Host job substrate rather than separate renderer paths.

## Firefox version pin

The renderer deliberately pins Playwright 1.61.1 / Firefox 151.0. CI with Playwright 1.63.0 / Firefox 155.0 repeatedly reached Juggler and then failed to launch the tab subprocess with `SIGSEGV`; private shared memory did not change that outcome. The older exact pin is a compatibility response to a current upstream Firefox regression, not a relaxation of Semwright sandbox policy.

## Firefox sandbox layering

The certified Ubuntu path runs the Playwright-pinned Firefox build inside the already-required Driver Host Bubblewrap + Landlock boundary. Node is the SHA-pinned Host tool; Firefox is resolved canonically from the explicitly executable owner runtime bundle and cannot escape that root. The complete browser bundle is not yet claimed as an immutable Host-attested artifact. Firefox's nested content sandbox is disabled with fixed helper-owned environment settings only after the outer Semwright sandbox marker is present; this avoids nesting a second sandbox authority while keeping AppArmor, Landlock, explicit filesystem grants, process limits and `network=false` authoritative.

Chrome-for-Testing/Chromium was evaluated first. Multiple exact CI builds aborted with `SIGTRAP/int3` before a usable automation endpoint appeared, even after isolated AppArmor, task-budget and address-space experiments. The production route therefore fails away from that browser rather than weakening Semwright's sandbox.

Running `render.mjs` directly outside the Driver Host boundary fails closed.

## Launch film

Normal PR CI renders a short opaque sequence, a two-frame alpha sequence and exercises cancellation through the real Driver Host. It uploads only small evidence.

The manual `Motion Canvas launch film` workflow renders the complete 1,560-frame / 52-second 1920×1080 project on an ephemeral GitHub runner. A fixed FFmpeg argument vector converts the validated PNG sequence to a mezzanine because the current MLT driver imports bounded media files rather than image sequences. The real MLT provider then assembles video + deterministic WAV and performs the final H.264/AAC encode. H.264 uses MLT's synchronous offline path (`real_time=0`) with two libx264 codec threads; lossless/audio profiles retain the certified no-drop async path (`real_time=-1`, `threads=2`). CI demonstrated that enabling MLT H.264 frame-worker parallelism can terminate with SIGSEGV, so the driver keeps scheduling conservative and confines throughput tuning to the encoder. H.264 uses CRF 18 with the `veryfast` preset. The pinned `melt` subprocess is bounded to the same 4 GiB address-space and 300 CPU-second ceilings already authorized by Driver Host; `ffprobe` retains a smaller 1 GiB / 30-second tool budget. These are ceilings, not reservations.

The final workflow artifact contains the MP4, poster, seven review frames, ffprobe evidence, SHA-256 sums and the real `DEMO_TRACE.json`. Full frame sequences and browser profiles are not persisted as repository content.
