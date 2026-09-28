# Reproduce the cross-application proof

**Verified branch checkpoint:** `2bde9f0f9d78460a2b5bbfe9ca3f08688ce33b6c`,
[Actions run 36401772211](https://github.com/seradotcom/semwright/actions/runs/36401772211).
The route, source-game acceptance and exported-pack acceptance all passed.
The [retained receipt](proofs/2bde9f0-receipt.json) records 332 semantic operations,
matching export/handoff digests, observed tail motion and both passing gameplay
records. This is a branch proof, not a merged public release.

Run the **Tideling real cross-app proof** GitHub Actions workflow on the revision to
be evaluated. It installs hash-pinned Blender 4.5.14 and Godot 4.7.2, builds both
drivers, the sandbox helper, CLI and broker from that same revision, and executes
`automation/cross_app.py`. Compilation and acceptance run on the hosted runner.

The harness installs the directly authored game as an isolated baseline. It deletes
the reference Bluegold geometry and species resource from that copy, then records
the seven remaining species and their hashes. Pairing and owner configuration files are private, temporary, and
excluded from uploaded evidence. No unavailable consent-sensitive command is
reclassified to get a passing result.

After the baseline is frozen, the recorded semantic route performs these steps:

1. Discover and describe the Blender producer capability.
2. Create BlueGoldFish, four materials, three bones, weighted geometry and a keyed
   swim action using typed capabilities through the CLI and broker policy layer.
   Resolve fresh revision-bound references after mutations.
3. Export only its named collection with `driver.blender.export.glb`. Verify the
   GLB digest, then use `artifact.handoff` between explicitly granted roots.
4. Authenticate the Godot EditorPlugin, discover the rescan capability, rescan and
   inspect the imported PackedScene.
5. Create a new typed species resource through `resource.duplicate`, then patch its
   identity, asset, stage two eligibility, dimensions, behavior and spawn weight.
   Inspect all saved values. The game discovers saved species resources, so this
   adds a species without a prewired code entry.
6. Exercise the resulting game with the external acceptance script. The candidate
   must come from the real spawn table and have a playing swim animation with observed tail-bone motion, tier
   group and collision layer. An overlap must fail at stage one; consuming ordinary
   prey through physical collisions must grow the player; the candidate must then
   be consumed and increase score.
7. Export the resulting game pack, then repeat the full runtime acceptance using
   the packaged executable with an explicit `--main-pack`. The external harness
   lets audio finish before orderly shutdown; it is not included in the package.
   Retain the portable Linux package, both gameplay records and source SHA.

The acceptance script positions fish and supplies ordinary prey to make the stage
boundary reproducible. It does not author the candidate or directly assign growth.
This is a deterministic integration check, not an organic human playthrough.

Inspect the uploaded `Tideling-cross-app-<SHA>` artifact:

- `operations.jsonl` and `descriptors.jsonl`: arguments, observed results, request
  identifiers and actual driver/policy execution provenance.
- `producer-discovery.json` and `consumer-discovery.json`: discovered surfaces.
- `BlueGoldFish.glb` and `BlueGoldFish.tres`: the resulting model and saved resource.
- `gameplay.log`, `proof-export.log`, `proof-portable-smoke.log`: runtime evidence.
- `RECEIPT.json`: produced only after all assertions and package checks pass.
- `Tideling-BlueGold/`: playable result with licenses, source SHA and file hashes.

A failed run retains logs but must never be presented as a passing receipt. The
baseline assets and synthesized audio were authored directly, before this proof.
A successful branch run proves the cross-app route on that branch. Golden art and
a clean merged-SHA public proof are separate acceptance gates in `ACCEPTANCE.md`.
