# Semantic Motion Authoring v1

The Film is a typed authoring model above, not instead of, video-domain and the managed Motion Canvas model. Sequence/Beat/Shot labels describe declared narrative intent; they do not claim automatic creative direction. Text, media, paths, annotations, captions, layers, native layout and output profiles remain explicit. Twelve archetypes and forty closed grammar realizations are parameterized by data. No JavaScript, shader, command or URL field is accepted.

## Time

The bounded rational STN solver represents starts and ends with exact rational difference constraints. Hard duration bounds, containment, cue anchors, start/end equalities and precedence gaps are checked before mutation. A negative cycle returns named conflicting constraints. Preferred durations are attempted in descending explicit priority and ascending span ID; infeasible preferences are reported, not silently forced. Tie-breaking follows stable graph/input order. The solver permits at most five million relaxations and 128 spans; exhaustion is an error, not a made-up plan. Delivery must end exactly at a frame boundary.

## Native realization

`integrations/composition/motion/native.ts` was recovered byte-for-byte from the earlier isolated artifact (SHA-256 af9af96ac3b5b47249f5f1a983eee66701eaf0a2349b7c15b83d949f0c0808ae before later reviewed changes). It uses Motion Canvas 3.17.2 Layout/signals/generators and native node types, not coordinates sent to an application UI. Font, asset and frame readiness are observed on the renderer side. Generated code only imports this fixed implementation and typed serialized data. Native integration/typechecking belongs to subsequent compiler/renderer commits; the portable fixture suite is not native acceptance.

## Grammar semantics

Fade and slide operate on alpha/position; width reveal/conceal changes native layout width; draw/erase changes path extent; settle includes rotation; emerge changes scale and alpha; resolve changes tracking and alpha. Connect binds native path endpoints. Group/separate/compare organize explicit subjects. Replace crossfades at shared position; swap snapshots peers before mutation. Shared-element carries position and scale; carry-forward retains a peer offset; match-position and match-scale affect only the named relation. Camera following, topology-checked path morph, progressive disclosure, step-through, path tracing, local-region highlighting, annotations, zoom context, explode structure, formatted counters, bar growth/data change, code selection/diff, text emphasis, stagger and hold each lower to typed operations. There are no aliases registered merely to inflate a count.

Each invocation uses one coherent native starting snapshot. Overlapping writes to the same underlying signal are rejected; source world/local coordinate aliases share conflict keys. Cross-shot references must resolve in the same native sequence. Cross-sequence carry requires an explicit asset/state handoff, not an invented live reference. Seek always replays native generator state; no unproved incremental cache is enabled.

## Evidence

The streaming verifier consumes typed observed frame rows, not reserialized intent. Reports retain actual range, sample/exhaustive coverage, observation/render/artifact digests, units and common ValidationReport. Safe-area, native text, font readiness, truncation and ratio checks use native readback. An overlap failure is deterministic only for the explicit no-overlap constraint with compatible observed rectangle geometry; ambiguous intersections remain UNKNOWN. A draw with alpha is not proof of visible pixel contribution; minimum pixel-visible time remains UNKNOWN when the native probe cannot establish it. Motion/geometry measurements do not certify good taste, comprehension or speech intelligibility.

A partial range cannot validate a whole Film. Authoring projects currently use a conservative replay policy: a render range must begin at frame zero because arbitrary seek/checkpoint equivalence for stateful generators has not been independently proven. Legacy low-level projects retain their existing bounded range behavior. This is the M08 fallback, not a claim that partial rendering is impossible upstream.

Native Broker/Driver Host acceptance includes one logical Film re-planned and rendered at 16:9, 9:16 and 1:1. The test keeps subject IDs and cues stable, checks the actual artifact manifest dimensions, and requires fresh Composition verification for each profile. Those results belong to the exact CI SHA that runs the native test; this document alone is not evidence.

Native acceptance, compositing and final AV master verification remain separate gates.
