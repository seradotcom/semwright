# Runbook

## 1. Pair and inspect

Use `driver.figma.document.status`, then `composition.inspect`. Record document ID, session ID, generation, revision, editor type, plugin build and driver commit.

## 2. Bootstrap only missing design primitives

The requirements are intentionally names, not hidden IDs. Resolve current local primitives first.

For missing primitives, use the ordinary explicit Figma surfaces:
- variable collection/create/set-value;
- style create/patch;
- component/text/layout primitives.

Record every write and refresh the revision after bootstrap.

## 3. Plan and apply

Submit `landing.spec.json` to `composition.plan`. Preserve the returned plan exactly. Inspect its digest, base state, resolved design refs, risk and required scope.

Apply with `composition.apply` at the same revision. A conflict means re-inspect/replan; do not replay blindly.

## 4. Observe

Call `composition.measure` on desktop and mobile. Confirm native TextNodes/instances/Auto Layout and the real frame widths.

## 5. Validate and repair

Call `composition.validate` with the same spec and a findings budget within the spec budget. The fixture intentionally declares 24 px minimum title/body gap while desktop/mobile hero parents initially use 16 px.

Pass the fresh findings to `composition.repair.plan`. Apply only the returned deterministic bounded repair after normal policy authorization. Re-measure and revalidate.

## 6. Visual verification

Call `composition.verify` for both roots. Preserve artifact metadata. Use the PNG only for visual judgment; use semantic state for identity and deterministic checks.

## 7. Follow-up mutation

Change one semantic/design-system property (for example the primary color variable or a section spacing value), observe the native document response, and verify the affected node.

## 8. Stale-plan test

Attempt to apply the original pre-repair plan after the revision has advanced. Expected result: stale-reference/conflict, never silent replay.

## 9. Evidence

Write exact PASS/FAIL/UNEXECUTED/BLOCKED status to `benchmark-results.json` and the final closeout report. Never reinterpret fake-runtime evidence as real Figma acceptance.
