# G transport bootstrap for the integrated candidate

Original suite: `a88e80f9de4aa2883b233d009a4b41ecdde08a0b`.

Runtime unification #201 changed the public Godot runner configuration and moved
Blender execution to protocol v8 Host-owned sessions. This G-only branch adapts
those bootstrap declarations and builds the exact candidate's session runner.
It preserves the existing fixture authority: workspace, pinned Blender runtime,
font configuration and private scratch. No network permission is added.

The full oracle identity changes because executable bootstrap source changes.
A new suite must be frozen before the combined campaign. This is not an old-suite
retest or a claim that old findings certify the new transport. The registry,
mutants, oracle implementation and Blender attack bodies are byte-identical;
`reports/I_TRANSPORT_BOOTSTRAP.json` records their hashes. All product lanes
must still use one explicit full `combined_candidate_sha`. R16 remains open.

The lint cleanup removes three unused local assignments from `native_apps.py`.
The native calls, result checks and case expectations are unchanged. This source
change is included in the new suite identity before the combined campaign.

## AV API compatibility on the integrated candidate

Campaign `37031369266` blocked all 35 AV cases at compilation: the original
probe predates command-proof vectors, post-encode PCM handoffs, and raw sync
measurement receipts. This is a harness build failure, not passing evidence
or a product finding. The candidate remains
`f0f4805d5e4dce51e5f91c8c86fef6f83bc94631`.

The AV fixture now supplies typed command descriptors, a distinct synthetic
post-encode WAV bound to its encoded master, and an artifact-bound relative
handoff. Sync receipts contain raw measurements so the coordinator computes
the verdict. G-AV-017 removes exhaustive audio coverage, which yields Unknown
under its original full-scan requirement, and still asserts rejection. Case
IDs, registry expectations and all other case mutations remain unchanged.
These are synthetic contract fixtures and do not certify native AV execution.

This executable adaptation changes the oracle identity. A new immutable suite
and complete combined campaign are required; the first campaign is retained
as diagnostic evidence. No blocked result is converted into a pass.
