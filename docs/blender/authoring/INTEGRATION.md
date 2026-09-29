# E integration map

## Frozen integration inputs

- resumed main / merged GLB baseline: `7a3bae71144bf2c2278b34fc5743e0ceed6dddd1`
- A C0 contract: `26602e4b25929be869d69ef28fef4dd9713180d7`
- C P0 publication: `6ee52b428310370d3ad438a13964086a63f48367`
- F consumed head: `42204ac6a6f3c66ba66de5adfe689d8633bb7c74`
- GLB source head from PR #154: `74671c11dda2133ce6af939896c49cdbb6ba47d5`

E dependency reconciliation commit is `995d856968a5f1bf16739d11ca122825bf2b9fc1`. E model source is `4dbee6e462275cb0b97b95f86eff605986a436ec`; native/effects source is `2d87f2f3df6e931090ace10bd268a211f6459ff6`. PR: #175.

## Runtime route

`composition.inspect` observes bounded source RNA. `composition.plan` builds typed Blender operations on A's generic `PreparedPlan`, pins F's contract digest and stores the complete plan in A's PlanVault. `composition.apply` checks the current native base, reserves the attempt, executes fixed operations through the existing private bridge, independently reads source state, evaluates F evidence, records the attempt, and returns A's `VerificationReport`.

`composition.measure` is explicit because evaluated depsgraph work can be expensive and can evaluate native data. It marks coverage as source-only or single-frame and returns self-intersection UNKNOWN because no narrow-phase method exists yet.

`composition.persist` creates a new blend library file. `composition.reopen` is only meaningful in a fresh driver process; the native E2E is responsible for proving writer/reader process IDs differ. GLB export remains `driver.blender.export.glb`.

## Consumer boundaries

C receives host-resolved logical identities and a candidate ExecutionReceipt, then its own registered adapter admits it. F receives native observations through its compiled EvidenceAdapter. D receives an artifact, never Blender write authority. A remains owner of Composition/vault/report types.

## CI

`.ci/blender-authoring.json` is an enum selector committed with the source SHA. `.github/workflows/blender-authoring.yml` checks `GITHUB_SHA == git rev-parse HEAD`. `blender-model` is portable contract/model feedback; `blender-native-authoring` provisions pinned Blender 4.5.14 and exercises Broker/Host/native save/reopen/export. A changed source always receives a new push/run rather than rerunning an old SHA.
