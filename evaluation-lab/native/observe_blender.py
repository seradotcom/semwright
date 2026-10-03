"""Fresh native observer, independent of direct helper and author metadata.

Writes a report outside the source project, without saving or exporting a scene.
It checks geometry, evaluated animation, material values and actual skin binding.
"""
import hashlib
import json
from pathlib import Path
import sys
import bpy


def observe(spec, source, kind):
    if kind == "blend":
        bpy.ops.wm.open_mainfile(filepath=str(source))
    elif kind == "glb":
        bpy.ops.object.select_all(action="SELECT")
        bpy.ops.object.delete(use_global=False)
        # Blender's importer normally adds an Icosphere bone display object in
        # glTF_not_exported. Disable that observer-owned visual helper so exact
        # mesh counts describe the artifact, without filtering any artifact mesh.
        assert "FINISHED" in bpy.ops.import_scene.gltf(filepath=str(source), disable_bone_shape=True)
    else:
        raise ValueError("Unknown native source kind")
    meshes = sorted((obj for obj in bpy.context.scene.objects if obj.type == "MESH"),
                    key=lambda obj: obj.name)
    arms = [obj for obj in bpy.context.scene.objects if obj.type == "ARMATURE"]
    checks = {"mesh_count": len(meshes) == spec["segments"],
              "editable_vertices": all(len(obj.data.vertices) >= 8 for obj in meshes),
              "native_rig": len(arms) == 1 and {"root", "hinge"}.issubset(arms[0].data.bones.keys())}
    widths, colors, bindings = [], [], []
    bpy.context.scene.frame_set(1)
    for obj in meshes:
        vertices = [obj.matrix_world @ vertex.co for vertex in obj.data.vertices]
        widths.append(max(v.x for v in vertices) - min(v.x for v in vertices))
        materials = [mat for mat in obj.data.materials if mat is not None]
        if materials and materials[0].use_nodes:
            bsdf = materials[0].node_tree.nodes.get("Principled BSDF")
            colors.append(list(bsdf.inputs["Base Color"].default_value) if bsdf else [])
        else:
            colors.append([])
        bindings.append(any(mod.type == "ARMATURE" and mod.object in arms for mod in obj.modifiers)
                        and "hinge" in obj.vertex_groups)
    checks["geometry_width"] = bool(widths) and all(abs(w-spec["width"]) < 0.002 for w in widths)
    checks["material_color"] = bool(colors) and all(len(c) == 4 and all(abs(a-b) < 0.003 for a,b in zip(c, spec["color"])) for c in colors)
    checks["skin_bindings"] = bool(bindings) and all(bindings)
    angles = []
    if arms and "hinge" in arms[0].pose.bones:
        bone = arms[0].pose.bones["hinge"]
        for frame in (1, spec["duration_frames"]):
            bpy.context.scene.frame_set(frame)
            angles.append(bone.matrix_basis.to_euler().y)
    checks["evaluated_animation"] = len(angles) == 2 and abs(angles[0]) < 0.003 and abs(angles[1] - spec["rotation"]) < 0.003
    return {"schema_version": 1, "oracle": "H-fresh-native-blender-v1",
            "native": True, "source_kind": kind,
            "source_sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
            "checks": checks, "widths": widths, "colors": colors, "angles": angles,
            "outcome": "PASS" if all(checks.values()) else "FAIL"}


def main():
    spec_path, source, kind, report_path = sys.argv[sys.argv.index("--")+1:]
    report = observe(json.loads(Path(spec_path).read_text()), Path(source), kind)
    Path(report_path).write_text(json.dumps(report, indent=2)+"\n")
    if report["outcome"] != "PASS":
        raise AssertionError("H native observer rejected scene: " + json.dumps(report["checks"]))


if __name__ == "__main__":
    main()
