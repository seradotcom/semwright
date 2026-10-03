"""Declared negative oracle control; never used in a productivity attempt."""
import sys
import bpy

source, target, mutation = sys.argv[sys.argv.index("--")+1:]
bpy.ops.wm.open_mainfile(filepath=source)
if mutation == "missing_animation":
    bpy.data.objects["Mechanism"].animation_data_clear()
elif mutation == "wrong_material":
    bpy.data.materials["Surface"].node_tree.nodes.get("Principled BSDF").inputs["Base Color"].default_value = (1, 0, 1, 1)
elif mutation == "wrong_geometry":
    bpy.data.objects["Part_00"].scale.x *= 2
else:
    raise ValueError("Unknown declared control")
bpy.ops.wm.save_as_mainfile(filepath=target)
