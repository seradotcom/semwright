# TIDELING

An original 2.5D reef survival demo and a real Blender → Godot Semwright proof.
One reef, three hero forms, seven baseline species, dash, combo and a three-minute run.
The BlueGold edition adds an eighth species created through the recorded broker route.

[Download the Linux demo and proof bundle](https://github.com/seradotcom/semwright/releases/tag/tideling-demo-v0.1.0).
The release contains the executable archive, a gameplay preview, operation trace,
source SHA, acceptance receipts and checksums. `RELEASE.json` is the authoritative
completion record; a development branch or pending workflow is not a release.

Extract `Tideling-BlueGold-Linux.tar.gz`, then launch
`Tideling-BlueGold/Tideling.x86_64`. Keep `Tideling.pck` beside it.
Linux x86_64 with OpenGL 3.3 is required. Play is local; no account or cloud service
is required by the game.

WASD/arrows or left stick move; Space/A bursts; Enter starts; Esc/Start pauses;
M toggles audio. R retries after the ending. Nearby brackets mark edible fish;
diamonds mark hunters. BlueGold becomes edible at growth stage two.

For source development, use Godot 4.7.2:

```sh
godot --headless --editor --path demos/tideling/project --import
godot --path demos/tideling/project
```

The source baseline keeps its reference Bluegold species disabled. The proof harness
removes that reference model and resource from an isolated copy before constructing
the new species. No direct Blender Python or Godot authoring call counts as a
Semwright operation. See [PROOF.md](PROOF.md) for the complete route and
[ACCEPTANCE.md](ACCEPTANCE.md) for evidence and limits.

The GLBs contain original geometry, materials, skeletons and animations.
`authoring/build_assets.py` reproduces them in Blender 4.5.14 (`-- --all`).
All fish, environment geometry and synthesized audio are original to this demo.
Fraunces title font is distributed under the bundled SIL Open Font License.
All compilation, rendering and acceptance run in GitHub-hosted Actions.
