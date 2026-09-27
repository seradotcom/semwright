# Blender semantic completeness verification

Authoritative heavy verification runs in GitHub Actions. Local development is intentionally limited to source editing, `cargo fmt`, `git diff --check`, syntax checks and small read-only audits so the workstation does not accumulate Blender downloads, Rust targets, fuzz corpora or render artifacts.

Frozen implementation baseline for this semantic-completeness mission:

`e543a6bed1497840efdb06ce0f9b96748e9529a1`

## Dedicated Blender acceptance

The **Blender semantic driver** workflow must:

1. download Blender 4.5.14 LTS from blender.org and verify the pinned SHA-256;
2. configure the scoped Driver Host Bubblewrap/AppArmor path without disabling the host userns restriction;
3. generate `RNA_COVERAGE.json` from that exact Blender runtime;
4. enforce zero unmapped persistent IDs, zero missing relation targets and the authoring/UI boundary gates;
5. run bounded semantic security/adversarial-ref tests;
6. compile the Rust driver and production sandbox;
7. validate the framed Driver Protocol handshake/catalog;
8. execute the real DriverProvider against sandboxed Blender;
9. exercise stale refs, typed mutation, deep relations and the major authoring overlays;
10. render a real PNG, reload it through scoped asset semantics and save a real `.blend`;
11. upload coverage and live logs as retained evidence.

The live acceptance intentionally covers Blender 4.5-specific Action/F-Curve APIs rather than relying on newer Blender APIs.

## Final PR acceptance

Before merge, the actual PR head must also complete the repository-wide required checks once: Quality/MSRV/source contracts, dependency/coverage/fuzz, platform suites, supply-chain/packaging and Native Integration. Iteration uses the dedicated Blender workflow to avoid wasting shared GitHub Actions capacity.

The full RNA matrix stays in the workflow artifact. A small checked-in coverage summary may record the final Blender version, coverage counts and SHA-256 of that artifact after the final green run.
