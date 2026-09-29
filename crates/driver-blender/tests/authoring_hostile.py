"""Hostile native fixtures for Blender export preflight.

The harness deliberately injects state directly into a disposable factory-startup Blender.
Those writes are test setup, never attributed to Semwright authoring.
"""
import json
import sys
import tempfile
from pathlib import Path

import bpy

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "adapters" / "blender"))
from semwright_blender.commands import Commands  # noqa: E402
from semwright_blender.validation import CommandError  # noqa: E402


def reset():
    bpy.ops.wm.read_factory_settings(use_empty=True)


def mesh_object(collection, name="Subject"):
    mesh = bpy.data.meshes.new(name + "Mesh")
    mesh.from_pydata(
        [(-0.5, -0.5, 0), (0.5, -0.5, 0), (0.5, 0.5, 0), (-0.5, 0.5, 0)],
        [],
        [(0, 1, 2, 3)],
    )
    obj = bpy.data.objects.new(name, mesh)
    collection.objects.link(obj)
    return obj


def fixture():
    collection = bpy.data.collections.new("HostileExport")
    bpy.context.scene.collection.children.link(collection)
    return collection, mesh_object(collection)


def denied(calls, path, expected="PolicyDenied", animations=True):
    try:
        calls(
            "blender.export.glb",
            {"collection": "HostileExport", "path": path, "animations": animations},
        )
    except CommandError as error:
        assert error.code == expected, (path, error.code, expected)
        return
    raise AssertionError(f"hostile case exported unexpectedly: {path}")


def main():
    results = []
    with tempfile.TemporaryDirectory(prefix="semwright-hostile-export-") as work:
        # Native animation driver: exporter must not evaluate/implicitly trust it.
        reset()
        collection, obj = fixture()
        obj.driver_add("location", 0)
        calls = Commands(bpy, work)
        denied(calls, "driver.glb")
        results.append("active_driver_denied")

        # Constraint references an object outside the declared collection.
        reset()
        collection, obj = fixture()
        outside = bpy.data.objects.new("OutsideTarget", None)
        bpy.context.scene.collection.objects.link(outside)
        constraint = obj.constraints.new("COPY_LOCATION")
        constraint.target = outside
        calls = Commands(bpy, work)
        denied(calls, "external-constraint.glb")
        results.append("external_constraint_denied")

        # A supported modifier with an external pointer is still outside closure.
        reset()
        collection, obj = fixture()
        outside = bpy.data.objects.new("MirrorTarget", None)
        bpy.context.scene.collection.objects.link(outside)
        modifier = obj.modifiers.new("HostileMirror", "MIRROR")
        modifier.mirror_object = outside
        calls = Commands(bpy, work)
        denied(calls, "external-mirror.glb")
        results.append("external_modifier_dependency_denied")

        # ACTIONS export cannot silently inspect unrelated global Actions.
        reset()
        collection, obj = fixture()
        bpy.data.actions.new("UnrelatedAction")
        calls = Commands(bpy, work)
        denied(calls, "unrelated-action.glb", animations=True)
        results.append("unrelated_action_denied")

        # Unsupported material node graphs fail closed before native glTF export.
        reset()
        collection, obj = fixture()
        material = bpy.data.materials.new("UnsafeMaterial")
        material.use_nodes = True
        script = material.node_tree.nodes.new("ShaderNodeScript")
        script.mode = "INTERNAL"
        obj.data.materials.append(material)
        calls = Commands(bpy, work)
        denied(calls, "script-shader.glb")
        results.append("script_shader_denied")

        # Existing symbolic output cannot redirect the fixed artifact publication.
        reset()
        collection, obj = fixture()
        target = Path(work, "outside.glb")
        Path(work, "symbol.glb").symlink_to(target)
        calls = Commands(bpy, work)
        denied(calls, "symbol.glb")
        assert not target.exists()
        results.append("symbol_output_denied")

        leftovers = list(Path(work).glob(".semwright-export-*"))
        assert not leftovers, leftovers

    print(
        "BLENDER_AUTHORING_HOSTILE "
        + json.dumps(
            {
                "version": 1,
                "setup": "direct disposable Blender hostile injection",
                "product_under_test": "existing GLB exporter plus E dependency preflight",
                "results": results,
                "passed": len(results) == 6,
            },
            sort_keys=True,
        )
    )


if __name__ == "__main__":
    main()
