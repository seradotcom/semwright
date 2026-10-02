"""Fresh-Blender oracle for an E-produced GLB.

This verifier may inspect the output directly. It must not repair, rewrite, or supply
missing authoring state. Failure means the announced GLB semantics were not observed.
"""
import hashlib
import json
import struct
import sys
from pathlib import Path

import bpy

try:
    marker = sys.argv.index("--")
    source = Path(sys.argv[marker + 1]).resolve(strict=True)
    report_path = Path(sys.argv[marker + 2])
except (ValueError, IndexError):
    raise SystemExit(2)

data = source.read_bytes()
if not (20 <= len(data) <= 64 * 1024 * 1024):
    raise AssertionError("GLB artifact outside oracle byte budget")
magic, version, declared = struct.unpack("<4sII", data[:12])
if magic != b"glTF" or version != 2 or declared != len(data):
    raise AssertionError("artifact is not framed GLB 2")

# Factory-startup may contain a default scene. Removing it is oracle setup only.
for obj in list(bpy.data.objects):
    bpy.data.objects.remove(obj, do_unlink=True)

result = bpy.ops.import_scene.gltf(filepath=str(source))
if "FINISHED" not in result:
    raise AssertionError("fresh Blender did not import GLB")

objects = list(bpy.data.objects)
if len(objects) > 128:
    raise AssertionError("imported object budget exceeded")
meshes = [obj for obj in objects if obj.type == "MESH"]
armatures = [obj for obj in objects if obj.type == "ARMATURE"]

bones = sorted(
    {bone.name for armature in armatures for bone in armature.data.bones}
)
skin_bindings = [
    {
        "mesh": obj.name,
        "armature_targets": [
            modifier.object.name
            for modifier in obj.modifiers
            if modifier.type == "ARMATURE" and modifier.object is not None
        ],
        "vertex_groups": sorted(group.name for group in obj.vertex_groups),
    }
    for obj in meshes
]
materials = sorted(
    {
        material.name
        for obj in meshes
        for material in getattr(obj.data, "materials", [])
        if material is not None
    }
)

actions = []
curve_count = 0
key_count = 0
for action in bpy.data.actions:
    action_info = {"name": action.name, "curves": 0, "keys": 0}
    # Blender 4.5 actions are slotted/layered. Count only actual keyframe curves.
    for layer in action.layers:
        for strip in layer.strips:
            for bag in getattr(strip, "channelbags", []):
                action_info["curves"] += len(bag.fcurves)
                curve_count += len(bag.fcurves)
                for curve in bag.fcurves:
                    action_info["keys"] += len(curve.keyframe_points)
                    key_count += len(curve.keyframe_points)
    actions.append(action_info)

extras = sorted(
    {
        str(obj.get("sw_authoring_entity"))
        for obj in objects
        if obj.get("sw_authoring_entity") is not None
    }
)

checks = {
    "mesh_count": len(meshes) >= 2,
    "armature_count": len(armatures) >= 1,
    "expected_bones": {"base", "hinge"}.issubset(set(bones)),
    "skin_binding": any(
        row["armature_targets"] and "base" in row["vertex_groups"]
        for row in skin_bindings
    ),
    "animation": bool(actions) and curve_count > 0 and key_count > 0,
    "materials": len(materials) >= 2,
}
if not all(checks.values()):
    raise AssertionError("GLB semantic roundtrip required check failed: " + json.dumps(checks))

report = {
    "version": 1,
    "oracle": "fresh-blender-4.5-glb-import-v1",
    "source_sha256": hashlib.sha256(data).hexdigest(),
    "bytes": len(data),
    "required_checks": checks,
    "objects": [{"name": o.name, "type": o.type} for o in objects],
    "bones": bones,
    "skins": skin_bindings,
    "materials": materials,
    "actions": actions,
    "logical_extras": {
        "observed": extras,
        "verdict": "PASS" if {"rig", "body", "arm"}.issubset(set(extras)) else "UNKNOWN",
        "reason": None
        if {"rig", "body", "arm"}.issubset(set(extras))
        else "glTF extras identity was not fully reconstructed; required structural semantics still verified",
    },
    "godot_verified": False,
    "result": "PASS",
}
report_path.parent.mkdir(parents=True, exist_ok=True)
report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
print("BLENDER_GLB_ROUNDTRIP " + json.dumps({"result": "PASS", "sha256": report["source_sha256"]}))
