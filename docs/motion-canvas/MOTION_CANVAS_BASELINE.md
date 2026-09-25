# Motion Canvas upstream baseline

Observed 2026-09-23 via official npm metadata and upstream source/docs.

- `@motion-canvas/core`, `@motion-canvas/2d`, `@motion-canvas/vite-plugin`: **3.17.2**, MIT.
- `@motion-canvas/ffmpeg`: **3.17.2**, MIT; not needed for the image-sequence-first backend.
- 3.18.0-alpha.0 is a prerelease, not the stable baseline.
- The Vite plugin declares `vite: 4.x || 5.x`; use exact Vite 5.4.21 for compatibility.
- Node is pinned to 22.22.0; the runtime does not use the obsolete Node 16 minimum.
- Use `Code`, not deprecated `CodeBlock`. Camera is available in 3.17.2.
- Integer FPS; supported canvas spaces are sRGB and display-p3.

No supported standalone `motion-canvas render` CLI was found. Official rendering
is documented through the editor/browser. The implementation will use the
versioned Vite project/scene transforms, the public Renderer class, and the
public Plugin/Exporter interfaces. It will not click editor pixels.

The preferred controlled backend builds a self-contained browser bundle before
rendering, communicates over Playwright's process pipe, and writes PNG frames
through a bounded exporter binding. This avoids a runtime HTTP/Vite listener.
Actual compatibility and sandbox outcomes belong in VERIFY.md, not assumptions.

Sources:
- https://github.com/motion-canvas/motion-canvas/releases/tag/v3.17.2
- https://github.com/motion-canvas/motion-canvas/issues/1218
- https://motion-canvas.io/docs/rendering/
- https://motion-canvas.io/api/core/plugin/Plugin/
- https://motion-canvas.io/api/core/app/Exporter/
- https://github.com/motion-canvas/motion-canvas/tree/v3.17.2/packages/core/src/app
