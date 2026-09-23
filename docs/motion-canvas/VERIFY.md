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

`.github/workflows/motion-canvas.yml` runs three jobs:

- **domain**: complete driver compile, unit/property/security tests, Clippy, rustfmt and whitespace.
- **real-render-and-host**: exact Node/Motion Canvas install on an ephemeral runner, generated-project typecheck/Vite build, SHA-pinned runtime manifest, real Driver Host conformance, opaque render, transparent render/pixel evidence and cancellation.
- **fuzz**: bounded smoke for semantic parser, refs, animation validation, path validation, SVG boundary and codegen escaping.

The render test is accepted only through the real Driver Host with `network=false`; direct unsandboxed helper rendering is intentionally not an acceptance path.

## Full film

The expensive 52-second render is isolated in the manual `.github/workflows/motion-canvas-launch-film.yml`. It builds both first-party providers, renders 1,560 Motion Canvas frames through Driver Host, creates a fixed mezzanine, and uses the real MLT provider for final H.264/AAC assembly.

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

Pull-request CI performs locked Rust compilation, unit/property/security tests, clippy with warnings denied, formatting and whitespace checks. The dedicated runtime job installs the exact Node lockfile on an ephemeral runner, materializes a managed fixture, typechecks/builds generated source, exercises real Driver Host conformance, renders opaque and transparent PNG sequences, tests cancellation and records the 4 GiB address-space probe.

Six bounded Motion Canvas fuzz targets run in GitHub Actions. They cover the semantic project parser, reference decoder, animation validation, path validation, SVG boundary and code-generation escaping.

The separate manual launch-film workflow renders all 1,560 frames at 1920x1080/30fps, generates the deterministic 52-second original WAV bed, uses the MLT driver for final H.264/AAC assembly, validates final media metadata and uploads the MP4, poster, review frames and trace evidence.

A green result applies only to the exact commit named by the Actions run. Source-only compilation is not proof of successful browser rendering.
