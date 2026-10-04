# Semantic creation integration

The historical integration branch is `integration/semantic-creation-i-20261001`.
Subsystem source and delivered artifacts are retained while integration resolves
remaining cross-component work on an isolated branch.

The immutable candidate, run IDs and readiness state are bound to exact Git commits and hosted CI records. A working branch tip is not a final candidate. Readiness requires fresh evidence for one published full SHA; historical subsystem certificates retain their original source and scope.

## Consumed source ancestry

| Component | Source |
| --- | --- |
| Composition / AV | `65b773f4dd627b860358342f4d40a1ac532566d1` |
| Audio | `df2654bed6d2ac57d547846b69d16ea48b4a9ee3` |
| Project Graph | `77b34d8abad50f242c4c8494e280fe82d5cbcf55` |
| Project Graph C14 integration | `28eba9d733d254289255b33a9c14d409fad3e2e2` |
| Godot | `70bd7857e9700b6f03547a706ff0e6496ffd838e` |
| Blender | `f492f13a028f781d9ca55631764578f5b327eb1b` |
| Effect Conformance | `eadd5caf9b3f47f24158de530b87ad07e597f25e` |
| Original adversarial suite | `a88e80f9de4aa2883b233d009a4b41ecdde08a0b` |
| Runtime #201 | `733037145c374d28cb7d0e3d51dc76c64f223ad4` |

The runtime merge and native transport adaptation require new certification;
subsystem-only green runs do not certify the integrated runtime.

## Integration changes requiring acceptance

- Composition/Godot explicit fresh-child reconciliation preserves owner/root incarnation,
  the old ledger and aggregate budgets; foreign, stale and replayed authority is
  denied. See [the reconciliation contract](RECONCILIATION.md).
- The audio production Effect Conformance consumer uses the actual admitted decoded measurements and
  attempt through `validate_plan`, `collect`, `evaluate`. See
  [the audio consumer contract](AUDIO_EFFECT_CONSUMER.md).
- AV admission requires each audio check to carry native evidence naming its
  master digest. Contextual contract observations may omit a future artifact
  pin only alongside that native evidence with matching base, scope and
  exhaustive flag. Foreign artifacts and context-only checks fail. Effect Conformance strict
  artifact matching and evaluated observations remain unchanged.
- Blender/Godot/Motion/MLT use the integrated Host transport; audio retains its
  bounded shared SDK compatibility route and original CPU limits. Pinned font
  reads allow only the fixed font packages and retain filesystem containment,
  non-symlink regular-file checks and byte budgets. See
  [the native transport contract](RUNTIME_TRANSPORT.md).

## Integrated engineering closure

The engineering baseline is `cd518748f742025a251b78028613aa1b16919e73`.
Its certificates are source-scoped; promotion does not relabel them as tests
executed on a later commit.

| Scope | Hosted Actions run | Result |
| --- | --- | --- |
| Combined AV, decoded audio, exhaustive sync and C publication | [37077508380](https://github.com/seradotcom/semwright/actions/runs/37077508380) | PASS |
| Blender native authoring and export | [37074959691](https://github.com/seradotcom/semwright/actions/runs/37074959691) | PASS |
| Godot native authoring and cross-app consumption | [37075910269](https://github.com/seradotcom/semwright/actions/runs/37075910269) | PASS |
| Audio, three operating systems and native engines | [37077804879](https://github.com/seradotcom/semwright/actions/runs/37077804879) | PASS |
| Composition and native consumers | [37080028165](https://github.com/seradotcom/semwright/actions/runs/37080028165) | PASS |
| Project Graph, Broker, rebuild, fuzz and scale | [37080806793](https://github.com/seradotcom/semwright/actions/runs/37080806793) | PASS |
| Effects, native consumers, fault cases and mutants | [37081317102](https://github.com/seradotcom/semwright/actions/runs/37081317102) | PASS |
| Frozen adversarial suite against the combined source | [37074787747](https://github.com/seradotcom/semwright/actions/runs/37074787747) | 480/480 PASS |
| Global job disposition including corrected ARM64 fixture | [37088550848](https://github.com/seradotcom/semwright/actions/runs/37088550848) | PASS with explicit original-failure disposition |
| Reproducible engineering review package | [37088728581](https://github.com/seradotcom/semwright/actions/runs/37088728581) | PASS |

The original global run `37074828959` remains FAILURE. Its only failed job used
a cached UIA test snapshot after a bounded physical-point search. The complete
ARM64 job passed with a freshly observed snapshot in `37085834067`. The main
promotion incorporates that exact test-only correction and requires the affected
Windows workflow to pass on the promotion revision. Production source and the
root lockfile remain identical to the engineering baseline; changes since that
baseline are documentation, formal AV ancestry and this test fixture.

## Main promotion and deferred work

The completed subsystem/runtime integration was authorized for promotion to `main`; benchmark
work was deferred to a separate evaluation track. The historical evaluation branch is preserved
on `eval/semantic-productivity-lab` at `6fe1994d9ab25f44631ef8c41e9dad8f05aee0ca`;
no model comparison was executed, and that evaluation is not an admission gate for this integration.

The promotion branch is `integration/semantic-creation-main-promotion`. The actual promoted SHA and affected Windows run are recorded in GitHub's immutable commit/run records. The frozen engineering and evaluation deliveries retain their original identities.

This integration record predates the later R16 repository-review closure. The current review
status and retained evidence are recorded in [VERIFY.md](../../VERIFY.md) and
[the R16 evidence directory](../../verification/r16-closeout/README.md). Physical desktop and
interactive Windows residuals remain in [release blockers](../../RELEASE_BLOCKERS.md).
This development integration does not declare release readiness.

All builds, tests, native engines and packaging run on hosted Actions. Execution
is staged to limit heavy jobs; required checks are retained. No release,
demo production or R16 closure is claimed by this integration ledger.
