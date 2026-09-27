# Figma semantic authoring demo

This package exercises the production semantic-authoring contract with Semwright's existing Figma driver. It does not contain credentials, Figma session state, private files, or a pre-rendered result.

## Files

- `build_demo_spec.py` deterministically generates the desktop/mobile composition.
- `semwright-landing.composition.json` is the generated intent fixture.
- `convergence_demo.py` exercises plan → apply → validate → repair plan → repair apply → revalidate → verify → semantic edit → stale-plan rejection.
- `benchmark.py` runs the structural A/B/C benchmark.
- `structural-structural-benchmark-config.json` records fairness rules and fixed conditions.
- `expected-semantic-checks.json` lists machine-verifiable acceptance expectations.
- `DESIGN_BRIEF.md` is the shared creative brief.

## Regenerate

```sh
python3 demos/figma-semantic-authoring/build_demo_spec.py
git diff --exit-code demos/figma-semantic-authoring/semwright-landing.composition.json
```

The generated fixture currently contains 73 declared nodes, including 50 native text intents, and two profile roots: 1440 desktop and 390 mobile.

## CI evidence

The existing `Native application integration / figma-driver` job builds the real Rust driver and repository fake-Figma runtime, then runs both demo scripts. Results are written under `verification/native-ci/figma-semantic-*.json` and uploaded with the Figma evidence bundle.

Fake-Figma evidence proves protocol, identity, revision, structure, validation, repair, and artifact contracts. It does not prove live Figma visual quality.

## Live Figma acceptance

Use an authorized disposable Figma file and the normal Semwright gateway:

1. start the production Figma DriverProvider through Semwright;
2. pair the official Plugin API bridge;
3. use `semwright capabilities describe driver.figma.composition.plan` to confirm the current schema;
4. invoke planning and all mutations through `semwright capabilities execute` or `semwright execute`;
5. build the generated desktop/mobile spec section-by-section if capture quality matters;
6. measure and validate after each section;
7. apply only deterministic, unambiguous repair ChangeSets;
8. verify the final roots and preserve returned artifact metadata;
9. run the post-creation edit with exact current node refs;
10. record editor type, plugin build, Semwright commit, driver version, date, capabilities, and PASS/FAIL/UNEXECUTED.

Do not substitute Figma MCP, arbitrary plugin JavaScript, CDP, SVG page import, or manual behind-the-benchmark edits for this live acceptance.

## Intentional failure

The desktop hero-copy starts at an 18 px Auto Layout gap while the contract requires at least 24 px between heading and body. The convergence runner must observe the failure before repairing it. If the initial validation unexpectedly passes, the demo fails rather than fabricating a repair story.

## No visual score from fake runtime

The structural benchmark reports live visual quality as `UNEXECUTED`. A real Figma artifact must be reviewed before making any claim about polish, hierarchy, or brand fit.

