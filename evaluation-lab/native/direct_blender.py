"""Reusable native direct-arm example. It never serves as its own oracle.

Executed by pinned Blender in hosted development smoke. Future direct models may
use, modify or replace this module; strict semantic actors may not invoke it.
"""
import json
from pathlib import Path
import sys
import bpy


def build(spec):
    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.object.delete(use_global=False)
    mat = bpy.data.materials.new("Surface")
    mat.use_nodes = True
    rig = bpy.data.armatures.new("Mechanism")
    arm = bpy.data.objects.new("Mechanism", rig)
    bpy.context.collection.objects.link(arm)
    bpy.context.view_layer.objects.active = arm
    arm.select_set(True)
    bpy.ops.object.mode_set(mode="EDIT")
    root = rig.edit_bones.new("root")
    root.head, root.tail = (0, 0, 0), (0, 0, 1)
    hinge = rig.edit_bones.new("hinge")
    hinge.head, hinge.tail = (0, 0, 1), (0, 0, 2)
    hinge.parent = root
    bpy.ops.object.mode_set(mode="OBJECT")
    arm.select_set(False)
    for i in range(spec["segments"]):
        bpy.ops.mesh.primitive_cube_add(size=1, location=(0, i * 0.75, 1))
        obj = bpy.context.object
        obj.name = "Part_%02d" % i
        obj.scale = (1.0, 0.5, 0.5)
        bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
        obj.data.materials.append(mat)
        vg = obj.vertex_groups.new(name="hinge")
        vg.add(list(range(len(obj.data.vertices))), 1.0, "REPLACE")
        modifier = obj.modifiers.new("Deform", "ARMATURE")
        modifier.object = arm
        obj.parent = arm
        obj.select_set(False)
    bpy.context.scene.render.fps = 24


def revise(spec):
    color = spec["color"]
    mat = bpy.data.materials["Surface"]
    mat.diffuse_color = color
    mat.node_tree.nodes.get("Principled BSDF").inputs["Base Color"].default_value = color
    for obj in bpy.data.objects:
        if obj.type == "MESH":
            obj.scale.x = spec["width"]
    arm = bpy.data.objects["Mechanism"]
    arm.animation_data_clear()
    bone = arm.pose.bones["hinge"]
    bone.rotation_mode = "XYZ"
    bone.rotation_euler = (0, 0, 0)
    bone.keyframe_insert(data_path="rotation_euler", frame=1)
    bone.rotation_euler = (0, spec["rotation"], 0)
    bone.keyframe_insert(data_path="rotation_euler", frame=spec["duration_frames"])
    bpy.context.scene.frame_end = spec["duration_frames"]
    bpy.context.scene.frame_set(1)


def main():
    spec_path, project, export = sys.argv[sys.argv.index("--") + 1:]
    spec = json.loads(Path(spec_path).read_text())
    if spec["phase"] == "create":
        build(spec)
    else:
        bpy.ops.wm.open_mainfile(filepath=project)
    revise(spec)
    # Only the actor saves/exports. The independently launched observer never does.
    bpy.ops.wm.save_as_mainfile(filepath=project)
    bpy.ops.export_scene.gltf(filepath=export, export_format="GLB",
                              export_animations=True, export_skins=True)


if __name__ == "__main__":
    main()
