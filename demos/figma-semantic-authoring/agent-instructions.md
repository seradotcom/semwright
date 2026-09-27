# Dogfood agent instructions

Use only current Semwright Figma capabilities discovered at runtime. Do not reconstruct capability schemas from this file.

1. Inspect document/session status.
2. Call `composition.inspect` with bounded design-system discovery.
3. Compare the result to `design-system.requirements.json`.
4. Create only missing required primitives using existing explicit variable/style/component capabilities. Do not create an alternate design-system path inside `composition.apply`.
5. Call `composition.plan` with `landing.spec.json` and the current revision.
6. Review the ChangeSet: required scope must remain `driver:figma`, risk must remain reversible, and no deletes are allowed.
7. Apply the plan under normal Broker policy.
8. Measure both desktop/mobile roots.
9. Validate against the same spec. Expect the seeded hero spacing findings; do not edit the file manually to hide them.
10. Call `composition.repair.plan` using findings from the fresh validation, then separately apply the authorized repair plan.
11. Measure and validate again. Stop if evidence becomes UNKNOWN, revision changes, authority is denied, severity worsens, or no deterministic progress remains.
12. Verify desktop and mobile roots to artifact-backed PNG evidence.
13. Perform one follow-up semantic change using existing semantic/native capability surfaces, then re-inspect and verify it.
14. Attempt to reuse the pre-mutation plan and record the expected stale-reference rejection.

Aesthetic review is judgment, not a deterministic PASS. Screenshots are visual evidence, not object identity.
