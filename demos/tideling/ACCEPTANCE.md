# TIDELING acceptance ledger

Contract: the user's original single-reef, Blender → Godot demo brief. These rows
retain the complete requested sequence; passing a functional test does not grant
art approval or imply completion of a later wave.

| Wave | Implemented | Evidence / remaining gate |
|---|---|---|
| 0 Art direction | Original palette, warm expressive fish/cool reef, side camera | ART_DIRECTION.md, original authored reference render |
| 1 Hero | 3 modeled silhouettes, actual rig, idle/swim/turn/bite/dash actions | GLB inspection and native Blender/Godot import; independent final visual/interface review: SHIP; runtime validates Idle, Swim, Turn, Bite and Dash paths |
| 2 Visual reef | Blender flora/rocks/shells, depth layers, fog, light ribbons, suspended particles and moving plants | Six hosted states reviewed: unobstructed hero, distinct growth stages and readable UI; final small-demo visual/interface review: SHIP |
| 3 Movement | Keyboard/arrows, stick mapping, acceleration/deceleration, facing, dash and following camera | Fixed-step input acceptance measures response, coast, deadzone, analog movement, turn and dash. Physical hardware and subjective human feel are not certified |
| 4 Core loop | Consumption, thresholds, predators, combo and 180-second ending | 13 rules checks and real collision acceptance pass; pause, timed ending, retry and mute suite also passes in hosted CI |
| 5 Ecosystem | 7 enabled species, weighted spawning, pursuit/flee behavior and small-fish schooling | Actual spawn and tier behavior verified; visual hierarchy review passed for the prototype, schooling is exercised during the full input-bot run |
| 6 Polish | Original audio, three ambient layers, bite/growth particles, camera breath, HUD, mute and reduced-effects option | Six visual states reviewed for legibility; reduced effects does not disable every ambient motion |
| 7 Golden game | Versioned reference demo, exported Linux pack, visual SHIP review and full-run/input gates | Release acceptance is recorded in the published RELEASE.json; it is a small playable demo, not a commercial-quality certification |
| 8 Semantic surfaces | Named rig/bones/materials/collection, typed species resources, tier groups and collision profiles | New export capability added; native adapter/contract CI passes |
| 9 Cross-app proof | Fresh BlueGoldFish authored through typed broker commands; GLB handoff; new Godot species resource; spawn, animation and consumption | **PASS** on branch SHA `2bde9f0`: candidate absent at baseline; tail motion observed; stage-one overlap rejected; collision-fed growth unlocks consumption and score; repeated against exported PCK. See PROOF.md and hosted receipt |
| 10 Clean merged-SHA public proof | PR #155, clean-main proof workflow and versioned public release | The published RELEASE.json must identify the actual merged SHA and passing main-run receipt; no branch run satisfies this gate |

## Release record

The [versioned release](https://github.com/seradotcom/semwright/releases/tag/tideling-demo-v0.1.0) carries `RELEASE.json`, `RECEIPT.json`, the
Linux archive, preview and operation trace. Final completion requires a PASS receipt
classified `MERGED_SHA_CROSS_APP_PROOF`, a clean source tree, matching source SHA,
both source and packaged gameplay acceptance, and matching archive checksums.
A missing release or pending workflow does not satisfy these conditions.

The final visual reviewer returned **SHIP** for the six valid 1280 × 800 states:
no material visual/interface fixes remain for the small demo. Extra environmental
variety and commercial art ambition were not treated as demonstrated blockers.
Control and animation timing are covered separately by runtime acceptance.

## Observed checkpoints

- Full cross-app route and exported BlueGold game acceptance: https://github.com/seradotcom/semwright/actions/runs/36401772211 (`2bde9f0`).
- Baseline controls, animation, lifecycle, exported pack and six-state capture: https://github.com/seradotcom/semwright/actions/runs/36401780346 (`2bde9f0`, PR merge checkout).
- Prototype visual review: six fixtures from https://github.com/seradotcom/semwright/actions/runs/36401489681 (`e6af94d`); no material interface/legibility blockers. Fixtures do not prove human feel or golden art.

- Blender driver/real adapter export: https://github.com/seradotcom/semwright/actions/runs/36396464354 (4afe3c1).
- Godot rules, real controls/collisions and portable pack: https://github.com/seradotcom/semwright/actions/runs/36396813694 (24c7fe4).
- Same hosted game checks: https://github.com/seradotcom/semwright/actions/runs/36397681533 (fa31449).
- Lifecycle checks and portable package: https://github.com/seradotcom/semwright/actions/runs/36398256912 (0b5c40a).
- Cross-app failure retained: https://github.com/seradotcom/semwright/actions/runs/36397681592 (fa31449).
- Missing provenance failure retained: https://github.com/seradotcom/semwright/actions/runs/36398481511 (e868bc8).
- Cross-app animation failure retained: https://github.com/seradotcom/semwright/actions/runs/36399345793 (0c3184c).
- Animated baseline, species discovery, controls, lifecycle, pack and six visual states pass: https://github.com/seradotcom/semwright/actions/runs/36400796292 (fefc6c0).
- Full input-bot run on `709e586`: 180 seconds, stage 3, 53,200 points and timer victory.
  It supplies movement inputs to the ordinary ecosystem without injecting prey or setting growth.
  This establishes an automated completed run, not human playfeel or hardware certification.
- Earlier input-bot checkpoint: stage 3, caught after 125.77 seconds; retained as historical evidence.

`automation/visual_matrix.gd` deliberately stages later growth states for screenshots.
Those captures are visual fixtures, never claimed as an organic playthrough.
`automation/runtime_proof.gd` injects test specimens; the separate cross-app runtime
acceptance requires the new species to originate from the actual spawn table.

All new compilation, packaging and final acceptance runs occur in GitHub-hosted Actions.
Generated logs, pairing files, engine copies and import caches are not source assets.
The baseline was authored directly. Only recorded broker operations count as Semwright
work; unavailable consent-sensitive operations are not bypassed or reclassified.
