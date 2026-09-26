# Blender semantic completeness verification

Authoritative heavy verification runs in GitHub Actions. Local development is intentionally limited
to source edits, `cargo fmt`, `git diff --check` and Python syntax checks so the workstation does
not accumulate Blender downloads, build targets, fuzz corpora or render artifacts.

## Required final evidence

The dedicated **Blender semantic driver** workflow must:

1. download Blender 4.5.14 LTS from blender.org and verify the pinned SHA-256;
2. generate `RNA_COVERAGE.json` from that exact runtime;
3. run bounded semantic security/adversarial-ref tests;
4. compile the Rust driver and production sandbox;
5. execute the real DriverProvider against sandboxed Blender;
6. exercise typed mutation, stale refs, deep relations and major authoring overlays;
7. upload the coverage and live logs as retained artifacts.

The final PR also runs the repository-wide Quality, dependency/coverage/fuzz, platform,
supply-chain, packaging and Native Integration workflows before merge.
