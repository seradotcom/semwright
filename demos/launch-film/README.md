# Semwright launch film

This directory contains the versioned semantic source for the Semwright launch film. `semwright-motion.json` is authoritative; generated TS/TSX, frames and video outputs are disposable derivatives.

The primary film is 52 seconds at 1920×1080/30 fps. Normal pull requests run only a short render/cancellation smoke. The full film is built by the manual `Motion Canvas launch film` workflow so a 1,560-frame render and final encode do not consume the owner workstation or every PR runner.

The full build is designed as a self-hosted proof: the Motion Canvas driver renders a bounded managed project into validated PNG frames; a fixed CI preprocessing step may create a mezzanine when required by the current MLT driver’s file-import surface; the MLT provider performs final timeline/audio assembly and H.264/AAC export. Every provider action and non-provider helper is recorded separately in `DEMO_TRACE.json`; helpers are never mislabeled as Semwright capabilities.

No paid creative API is required. Sound is generated deterministically by `scripts/demo/generate-launch-sound.py`. The film does not contain fake terminal output or fake product screenshots.

## Development

Source-only checks validate JSON, asset manifests, recipe syntax, all seven managed fixtures and the semantic compiler. Real browser rendering, Driver Host confinement, fuzzing and the full film belong in GitHub Actions. Do not commit `node_modules`, browser profiles, PNG sequences, render temp directories, WAV intermediates or MP4 artifacts.

The manual workflow emits `launch-film-1080p.mp4`, `poster.png`, seven representative review frames, `final-ffprobe.json`, SHA-256 sums and the execution `DEMO_TRACE.json` as one GitHub Actions artifact.
