# Demo production handoff

Purpose: allow a later creative agent to produce a professional video without implementing missing infrastructure, inventing permissions or bypassing Semwright. This document is not a launch-film script.

Status at authoring time: infrastructure work is still being integrated. Do not start production until INTEGRATION.md contains one combined candidate SHA and READY_FOR_DEMO_PRODUCTION is true for the intended workflow.

## Prerequisites

The production environment must provide the exact candidate's:

- Semwright Broker/CLI/MCP entry point;
- Figma driver if the chosen workflow imports/edits Figma source;
- Motion Canvas driver and its pinned runtime/browser;
- audio provider/authoring stack announced by the verified B handoff;
- MLT video delivery provider/runtime;
- owner-configured filesystem/artifact grants;
- installed first-party Skills for the chosen agent/client.

Do not add shell, network, microphone, streaming or broad filesystem permission because a prerequisite is missing.

## Preflight

1. Record the Semwright/candidate SHA.
2. Use capability discovery/describe for every operation; do not copy argument schemas from this runbook.
3. Run Skill doctor/test against the combined catalog.
4. Confirm Motion, Audio, MLT/Delivery and artifact services are available under the intended owner/session.
5. Confirm required fonts/assets exist through explicit provider/grant paths and retain their digests/licenses.
6. Confirm output frame rate, dimensions, sample rate, channel layout and exact target duration before mutation.
7. Confirm no previous candidate/output pointer will be overwritten unless the owner explicitly opted into replacement.

## Production sequence

### Visual source

Use Figma only if it is part of the chosen creative workflow. Follow inspect → composition.plan → review → apply → measure → validate → bounded repair → verify. Export assets with provider artifact metadata intact.

### Motion authoring

Use driver.motion-canvas.composition.inspect and plan from the Film source. Apply only the server-issued fresh plan. Use render.start/status/result or the synchronous render.execute route according to the live catalog. Measure/validate/verify the exact rendered artifact.

Choose narrative, archetypes, timing and aesthetic decisions deliberately. The authoring runtime executes them; it does not invent good taste.

### Audio

Use the public audio capabilities/Skill delivered by B. Do not call private Ardour/Faust helpers or edit project files behind the provider. Preserve cues, sample/frame units, master/stem artifact digests, analysis method/version and unknown states.

### AV delivery

Create one AV plan using the shared cue graph and exact Motion/Audio artifact provenance. Do not silently retime narration or substitute a backend. Keep the audio provider token separate from its owner-configured filesystem locator. Bind that locator only from the verified public audio receipt/integration configuration, copy the final WAV through `artifact.handoff` using its exact SHA into the delivery root (current handoff limit: 64 MiB), and map the MLT `media` alias to that same storage. Then assemble with the MLT/delivery provider, analyze the encoded audio, and decode the final master for sync verification. Never parse a provider token as a pathname.

A successful intermediate render does not make the AV master ready.

## Verification required before accepting a master

- visual Motion verification PASS for all required checks in declared coverage;
- audio provider verification PASS on the rendered master;
- final encoded audio analysis PASS for the declared final-audio rules;
- decoded AV sync PASS using the predeclared cue/tolerance specification;
- exact final artifact digest and bounded publication manifest;
- no missing/UNKNOWN required rule hidden by an aggregate status.

Human creative review remains separate. Automated geometry/audio checks do not certify beauty, comprehension or brand quality.

## Change handling

A later semantic change invalidates dependencies rather than the whole world by default:

- visual-only asset/font/token change: replan/rerender affected visual dependencies; retain audio only if its dependency set is unchanged;
- mix/DSP change: retain valid visual artifacts, but render/analyze audio again and rebuild/reverify delivery;
- narration/timing/cue change: replan dependent Motion captions/beats and audio regions, then rebuild final delivery;
- runtime/compiler/provider/schema change: treat affected artifacts as stale even when source intent is identical.

Never reuse final encode verification across a new mux.

## Failure and recovery

On stale state: inspect and replan.

On partial mutation: record which effects completed; do not advertise the candidate as ready.

On transport loss after a non-idempotent call: observe/reconcile before any retry.

On cancellation: ensure the provider/Driver Host reports terminal cancellation and no published master pointer exists.

On UNKNOWN verification: gather stronger evidence or request human intervention; do not downgrade the rule.

## Outputs and cleanup

Keep generated source trees, browser profiles, frames, stems and intermediates out of Git unless a deliberately small fixture is being reviewed. Use private job/workspace outputs with bounded retention.

Publish only the verified manifest/pointer through the configured artifact route. Remove disposable test projects/sessions with their own authorized cleanup operations. Do not delete unrelated user work as compensation.

## Non-goals

This handoff does not contain a launch script, campaign copy, brand assets, marketing metrics or a promotional video. Those belong to the later creative production mission.
