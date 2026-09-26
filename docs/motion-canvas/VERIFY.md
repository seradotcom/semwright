# Motion Canvas verification

Frozen Semwright baseline: `a14abd8328e092a8227584750e47c38a77449ffa`. See `SEMWRIGHT_SNAPSHOT.md` for the one-time baseline/CI snapshot.

## Lightweight local checks

The owner workstation is not used for browser installation, fuzz campaigns or the full film. Safe source checks are:

```bash
cargo fmt --all -- --check
cargo check --locked -p semwright-driver-motion-canvas --all-targets
cargo clippy --locked -p semwright-driver-motion-canvas --all-targets -- -D warnings
git diff --check
```

Unit/property/security tests are small enough for CI and may be run locally when diagnosing code, but the authoritative evidence is GitHub Actions.

## Pull-request workflow

`.github/workflows/motion-canvas.yml` runs four verification layers:

- **domain**: complete driver compile, unit/property/security tests, explicit source/codegen/catalog/manifest/render-plan goldens, bidirectional semantic-registry/API-coverage contracts, project-variable and property-introspection/stale-ref contracts, launch-film recipe validation against the real capability catalog and JSON Schemas, a bounded 500-node/100-animation/50-edge stress compile, launch-source/audio provenance verification, Clippy, rustfmt and whitespace.
- **portable-compile**: locked Rust compile of the complete domain and Driver Protocol adapter on macOS and Windows runners.
- **real-render-and-host**: exact Node/Motion Canvas/Firefox install on an ephemeral Linux runner, runtime contract tests, executable coverage checks against the installed 3.17.2 `.d.ts` surface, generated-project typecheck/Vite build for both the legacy fixture and `semantic-complete`, SHA-pinned runtime manifest, real Driver Host conformance, opaque render, transparent render/pixel evidence and cancellation.
- **fuzz**: bounded smoke for semantic parser, refs, animation validation, path validation, SVG boundary and codegen escaping.

The render test is accepted only through the real Driver Host with `network=false`; direct unsandboxed helper rendering is intentionally not an acceptance path. The runtime job also verifies that the pinned Firefox build can initialize Canvas under the original 4 GiB virtual-address-space ceiling.

## Semantic completeness gates

`integrations/motion-canvas/runtime/tools/semantic-coverage.mjs` compares the checked-in matrices to the exact installed 3.17.2 typings. Current acceptance requires:

- all 25 public `@motion-canvas/2d` component exports classified;
- every own field of every public component `Props` interface classified, including per-arm classification for mixed union values;
- all remaining public 2D root modules classified: code (47 exports), curves (17), decorators (28), partials (31), scenes (3), utils (19) and `jsx-runtime` (4);
- all 15 root `@motion-canvas/core` exports classified;
- all 8 `ProjectSettings` fields classified;
- every public flow (13), transition (6) and tweening (60) function/constant classified;
- every managed/represented property mapping resolves through the Rust semantic registry;
- every registry semantic-storage property has an upstream coverage witness;
- every managed easing/transition in core coverage maps to a real compiler enum/function;
- the `semantic-complete` fixture validates/compiles in Rust and typechecks/Vite-builds against the exact installed Motion Canvas runtime.

The matrices may classify a surface as `compiler_managed`, `runtime_internal`/`runtime_utility`, `mixed` or `unsupported_by_design`; such entries require an explicit boundary rather than silently disappearing. A `mixed` property is accepted only when every union arm is itself managed, represented or explicitly excluded.

## Full film

The expensive 52-second render is isolated in `.github/workflows/motion-canvas-launch-film.yml`. It builds both first-party providers, renders 1,560 Motion Canvas frames through Driver Host, creates a fixed mezzanine, and uses the real MLT provider for final H.264/AAC assembly.

Success requires:

```text
launch-film-1080p.mp4: 1920x1080, 30 fps, ~52 s, video + audio
poster.png
7 representative review PNGs
DEMO_TRACE.json
final-ffprobe.json
SHA256SUMS.txt
```

The Actions artifact is the media delivery surface; the repository must remain free of frame sequences, browser profiles, node_modules, WAV intermediates and final MP4 binaries.

## Evidence discipline

A successful source build is not a real-render PASS. A real short render is not a launch-film PASS. The final report must reference the exact GitHub run/commit that produced each result and must not carry PASS status from an older SHA.

If an optional platform or tool is unavailable, the driver must report it fail-closed through `doctor`; verification must not bypass Driver Host to make a green result.

## CI evidence

Pull-request CI performs locked Rust compilation, unit/property/security tests, Clippy with warnings denied, formatting and whitespace checks. The dedicated runtime job installs the exact Node lockfile and Playwright-pinned Firefox on an ephemeral runner, materializes a managed fixture, typechecks/builds generated source, exercises real Driver Host conformance, renders opaque and transparent PNG sequences, tests cancellation and checks the pinned browser under the same 4 GiB ceiling used by the driver.

Seven bounded Motion Canvas fuzz targets run in GitHub Actions. They cover the semantic project parser, reference decoder, animation validation, path validation, SVG boundary, code-generation escaping and the version-pinned semantic property registry/value boundary.

The stress test emits a `MOTION_STRESS` line with measured validation/codegen microseconds and generated byte count for that runner. Those values are diagnostic measurements, not performance claims or fixed acceptance thresholds.

The separate launch-film workflow renders all 1,560 frames at 1920×1080/30fps, generates the deterministic 52-second original WAV bed, uses the MLT driver for final H.264/AAC assembly, validates final media metadata and uploads the MP4, poster, review frames and trace evidence.

A green result applies only to the exact commit named by the Actions run. Source-only compilation is not proof of successful browser rendering.
