# Semantic creation integration

The integrator owns branch `integration/semantic-creation-i-20261001`. All
original owners have stopped. Their source and delivered artifacts are retained;
the integrator resolves remaining cross-owner work on this isolated branch.

The current immutable candidate, run IDs and readiness state are published in
`semantic-creation/I.json` in the coordination directory. A working branch tip is
not a final candidate. Readiness requires fresh evidence for one published full
SHA; historical owner certificates retain their original source and scope.

## Consumed source ancestry

| Owner | Source |
| --- | --- |
| A | `65b773f4dd627b860358342f4d40a1ac532566d1` |
| B | `df2654bed6d2ac57d547846b69d16ea48b4a9ee3` |
| C | `77b34d8abad50f242c4c8494e280fe82d5cbcf55` |
| C14 integration | `28eba9d733d254289255b33a9c14d409fad3e2e2` |
| D | `70bd7857e9700b6f03547a706ff0e6496ffd838e` |
| E | `f492f13a028f781d9ca55631764578f5b327eb1b` |
| F | `eadd5caf9b3f47f24158de530b87ad07e597f25e` |
| Original G suite | `a88e80f9de4aa2883b233d009a4b41ecdde08a0b` |
| Runtime #201 | `733037145c374d28cb7d0e3d51dc76c64f223ad4` |

The runtime merge and native transport adaptation require new certification;
owner-only green runs do not certify the integrated runtime.

## Integration changes requiring acceptance

- A/D11 explicit fresh-child reconciliation preserves owner/root incarnation,
  the old ledger and aggregate budgets; foreign, stale and replayed authority is
  denied. See [the reconciliation contract](I_RECONCILIATION.md).
- B's production F consumer uses the actual admitted decoded measurements and
  attempt through `validate_plan`, `collect`, `evaluate`. See
  [the audio consumer contract](I_AUDIO_EFFECT_CONSUMER.md).
- AV admission requires each audio check to carry native evidence naming its
  master digest. Contextual contract observations may omit a future artifact
  pin only alongside that native evidence with matching base, scope and
  exhaustive flag. Foreign artifacts and context-only checks fail. F's strict
  artifact matching and evaluated observations remain unchanged.
- Blender/Godot/Motion/MLT use the integrated Host transport; audio retains its
  bounded shared SDK compatibility route and original CPU limits. Pinned font
  reads allow only the fixed font packages and retain filesystem containment,
  non-symlink regular-file checks and byte budgets. See
  [the native transport contract](I_RUNTIME_TRANSPORT.md).

## Required remaining closure

1. Certify native combined AV, including pre/post-encode audio, exhaustive sync,
   Broker publication and C divergence/restoration receipts.
2. Freeze one candidate and run complete E certification. Consume its exact
   artifact/run/source/digest through D's full cross-app certification and C
   receipts on that same source.
3. Complete B's full audio certification, the required Composition/native
   consumers and global quality, security, packaging and supply-chain checks.
4. Deliver the explicit immutable SHA to the separately frozen G suite. Every
   product lane runs against that source, preserving failed/blocked/not-run
   states. G's original attack registry and expectations remain intact.
5. Preserve the exact review baseline and evidence for the independent R16
   review defined in [the security review contract](../security-review.md).

All builds, tests, native engines and packaging run on hosted Actions. Execution
is staged to limit heavy jobs; required checks are retained. No main merge,
release, demo production or R16 closure is claimed by this integration ledger.
