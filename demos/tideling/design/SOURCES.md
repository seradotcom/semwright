# Source and license record

- Fish, coral, kelp, rocks, shells and anemones: original procedural mesh/rig/action authoring in `authoring/build_assets.py`. No stock or image-generated meshes.
- Audio: original deterministic synthesis in `authoring/build_audio.py`.
- Font: Fraunces variable font from Google Fonts (`ofl/fraunces`), retrieved 2026-09-28; source https://github.com/google/fonts/tree/main/ofl/fraunces ; SIL OFL reproduced in `project/assets/FONT-LICENSE.txt`.
- Blender API consulted for fixed glTF export: https://docs.blender.org/api/main/bpy.ops.export_scene.html . Execution target and evidence use Blender 4.5.14 LTS.
- Godot import design informed by official 3D scene pipeline: https://docs.godotengine.org/en/stable/tutorials/assets_pipeline/importing_3d_scenes/index.html . Execution target is 4.7.2 stable.
- Gameplay premise is the generic eat/grow/survive loop; characters, geometry, sounds, title treatment and level composition are original. TIDELING remains a provisional project name, not a trademark clearance claim.
- The portable checkpoint includes Godot's official engine binary and its MIT license; font license included separately. It is built on GitHub-hosted Actions and contains no Semwright pairing secret or proof harness.
- Godot 4.7.2 animation import renames loop-hint clips: https://github.com/godotengine/godot/blob/4.7.2-stable/editor/import/3d/resource_importer_scene.cpp . Runtime selection accepts the imported Swim name and explicitly enables its loop.
- Godot resource discovery preserves source resource names in exported packs: https://docs.godotengine.org/en/stable/classes/class_resourceloader.html#class-resourceloader-method-list-directory . Species are sorted before registration for deterministic ordering.
