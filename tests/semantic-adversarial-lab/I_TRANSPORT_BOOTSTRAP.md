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
