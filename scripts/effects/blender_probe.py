"""Fixed compiled-in fixture harness. Product Commands creates/saves/exports.
Direct RNA here is only the independent observer or a labeled injected mutant.
"""
import json
import os
from pathlib import Path
import sys
import bpy

sys.path.insert(0, "/src/adapters/blender")
from semwright_blender.commands import Commands

phase, case = sys.argv[sys.argv.index("--") + 1:]
root = Path("/work/data")
calls = Commands(bpy, str(root))

def project():
    hero = calls("blender.object.get", {"name": "Hero"})["object"]
    # Sorted native inventory is bounded by this isolated three-object fixture.
    members = sorted(o.name for o in bpy.data.collections["ExportScope"].all_objects)
    return {"hero": hero, "members": members}

if phase == "write":
    calls("blender.collection.create", {"name": "ExportScope"})
    calls("blender.object.create", {"name": "Hero", "primitive": "cube", "location": [0,0,0]})
    calls("blender.object.transform", {"name": "Hero", "location": [1,2,3]})
    calls("blender.collection.link", {"object": "Hero", "collection": "ExportScope"})
    calls("blender.object.create", {"name": "Excluded", "primitive": "cube", "location": [20,20,20]})
    before = project()
    export = calls("blender.export.glb", {"collection": "ExportScope", "path": "scope.glb", "animations": False})
    calls("blender.file.save", {"path": "scene.blend"})
    payload = {"projection": before, "export": export}
else:
    calls("blender.file.open", {"path": "scene.blend"})
    payload = {"projection": project()}
    if phase == "mutant":
        # Explicit fault injection AFTER product output. Not product authoring.
        # A visually invisible EMPTY outside the declared collection appears in
        # an otherwise valid native GLB. Membership oracle must reject it.
        outside = bpy.data.objects.new("InjectedOutside", None)
        bpy.context.scene.collection.objects.link(outside)
        for obj in list(bpy.context.selected_objects): obj.select_set(False)
        for obj in bpy.data.collections["ExportScope"].all_objects: obj.select_set(True)
        outside.select_set(True)
        result = bpy.ops.export_scene.gltf(filepath=str(root / "scope.glb"),check_existing=False,
            export_format="GLB",use_selection=True,export_cameras=False,export_lights=False,export_animations=False)
        if "FINISHED" not in result: raise RuntimeError("mutant exporter failed")
payload.update(schema_version=1,phase=phase,native_pid=os.getpid(),runtime=bpy.app.version_string)
Path("/work",phase+".json").write_text(json.dumps(payload,sort_keys=True))
