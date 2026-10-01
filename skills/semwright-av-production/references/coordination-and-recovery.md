# Coordination and recovery

## Dependency invalidation

Treat reuse as a dependency decision, not a filename/cache hit.

- A visual/font/token change may reuse audio only when the audio artifact's complete dependency set is unchanged.
- An audio mix/DSP change may reuse a valid Motion intermediate, but final mux, final-audio analysis and sync verification are new.
- A cue/narration/timing change invalidates every dependent Motion caption/beat and audio region/SFX timing.
- Runtime, descriptor, schema, compiler or provider-generation drift invalidates affected evidence even when intent bytes are unchanged.
- Final encode verification is never inherited across a new mux.

## Partial effects

The AV coordinator records every dispatched stage and its observed terminal status. If a later provider fails, already-created private candidates remain effects; they do not become a ready master and are not automatically deleted as rollback.

After transport loss on a non-idempotent operation, observe provider/project/artifact state before any new mutation. Cancellation is complete only when the native provider reports a terminal state.

## Synchronization evidence

The delivery decoder measures technical flash/impulse cue windows on the final encoded artifact. The common verifier re-evaluates raw detections against originally pinned cue times, offset/drift tolerances, uncertainty and confidence. Window coverage is not a claim about unrelated frames outside declared cue scopes.

## Publication

Keep final candidates private until all required reports are tied to the same plan and artifact digest. Owner-configured filesystem/artifact grants determine publication roots; plans and Skills cannot invent or widen them.
