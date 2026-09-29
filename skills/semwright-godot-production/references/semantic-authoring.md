# Semantic authoring

The managed flow is:

```text
inspect -> plan -> review -> apply -> measure -> validate
        -> optional repair.plan -> repair.apply -> verify
```

`GodotAuthoringSpec` is the managed intent for files Semwright owns. Stable logical IDs bind authored concepts to the current generated files/resources; native application refs are separate and may need reacquisition after restart.

The Behavior IR is intentionally bounded. It models typed literals/expressions, events, state transitions, timers/counters, movement/transform actions, managed spawn/despawn, animation/audio/UI actions, and allowlisted scene changes. It has no `run_code`, arbitrary method dispatch, shell, eval, remote load, or plugin installation escape.

Planning is side-effect free with respect to the target project. Applying revalidates the prepared base before publication. Incremental compilation writes only changed managed files. Unchanged generated resources keep their bytes and existing logical identity.

A plan belongs to the authenticated host owner and is single-attempt. Never copy a plan between sessions or recompute its digest after editing its body.
