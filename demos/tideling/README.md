# TIDELING

An original 2.5D reef survival game and a Blender → Godot Semwright proof in progress.
Source checkout: `semwright-worktrees/tideling`, branch `feat/tideling`.
Nothing in PharmacyOS, Parcel Lantern, Semantic Atelier or the Figma project is modified.

See `design/ART_DIRECTION.md` for the visual contract. Generated artifacts and evidence
are separate from authored sources. No direct Blender Python or Godot authoring call
counts as a Semwright operation. Public proof requires the actual broker route and a
clean merged source revision; the development branch cannot satisfy that final gate.

Run with an installed Godot 4.x:

```sh
godot --editor --path demos/tideling/project --import
godot --path demos/tideling/project
```

WASD/arrows or left stick move; Space/A bursts; Enter starts; Esc/Start pauses;
M toggles audio. R retries after the ending. Bluegold is disabled in the baseline.

The generated GLBs contain original geometry, materials, skeletons and animation.
`authoring/build_assets.py` reproduces them in Blender 4.5.14 (`-- --all`).
Title font: Fraunces, Google Fonts distribution, SIL Open Font License in assets/FONT-LICENSE.txt.
All fish/environment geometry and synthesized audio are original to this demo.

For the reproducible broker route and evidence interpretation, see [PROOF.md](PROOF.md).
The game workflow publishes a baseline Linux development package; the cross-app
workflow publishes its enabled BlueGold variant only after acceptance passes.
