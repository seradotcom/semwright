"""Source-bound native transform revision regression.

Ordinary Python uses RNA-shaped object fixtures and does not start Blender.
The explicit --native mode uses real Blender datablocks in a disposable process.
Neither mode claims Broker/Host admission, file persistence or UI acceptance.
"""
from __future__ import annotations

import ast
import copy
import hashlib
import json
import math
from pathlib import Path
from types import SimpleNamespace
import unittest
import uuid
import sys

NATIVE_BPY = None

ROOT = Path(__file__).resolve().parents[3]
SOURCES = (
    ROOT / "crates/driver-blender/src/authoring_native.py",
    ROOT / "crates/driver-host/src/windows_blender52_runtime/authoring_native.py",
)


class CommandError(Exception):
    def __init__(self, code, message):
        super().__init__(message)
        self.code = code


def load_runtime(path):
    tree = ast.parse(path.read_text(encoding="utf-8"))
    nodes = [node for node in tree.body
             if isinstance(node, (ast.FunctionDef, ast.ClassDef))]
    namespace = {
        "hashlib": hashlib, "json": json, "math": math, "uuid": uuid,
        "CommandError": CommandError, "Path": Path,
        "ISLAND": "sw_authoring_island", "ENTITY": "sw_authoring_entity",
        "MATERIAL": "sw_authoring_material", "TEXTURE": "sw_authoring_texture",
        "MARKER": "sw_authoring_observed_v1",
    }
    exec(compile(ast.Module(body=nodes, type_ignores=[]), str(path), "exec"),
         namespace)
    return namespace


class ObjectState(SimpleNamespace):
    def get(self, name, default=None):
        return {
            "sw_authoring_entity": "item",
            "sw_authoring_island": "owned",
        }.get(name, default)


def transform_values(obj):
    fields = ("rotation_quaternion", "rotation_axis_angle", "delta_location",
              "delta_rotation_euler", "delta_rotation_quaternion", "delta_scale",
              "location", "rotation_euler", "scale")
    return {"rotation_mode": obj.rotation_mode,
            **{name: list(getattr(obj, name)) for name in fields}}


def fixture(namespace):
    if NATIVE_BPY is not None:
        bpy = NATIVE_BPY
        # This explicit mode runs in a disposable native acceptance process.
        for collection in list(bpy.data.collections):
            if collection.get("sw_transform_revision_fixture"):
                for owned in list(collection.objects):
                    bpy.data.objects.remove(owned, do_unlink=True)
                bpy.data.collections.remove(collection)
        collection = bpy.data.collections.new("Owned")
        collection["sw_transform_revision_fixture"] = True
        collection[namespace["ISLAND"]] = "owned"
        bpy.context.scene.collection.children.link(collection)
        obj = bpy.data.objects.new("Owned", None)
        obj[namespace["ISLAND"]] = "owned"
        obj[namespace["ENTITY"]] = "item"
        collection.objects.link(obj)
        scene = bpy.context.scene
        scene.unit_settings.scale_length = 1.0
        scene.render.fps = 24
        scene.render.fps_base = 1.0
        scene.frame_set(1)
        runtime = namespace["AuthoringRuntime"](
            bpy, None, SimpleNamespace(changed=lambda: None))
        initial = runtime.snapshot("owned")
        collection[namespace["MARKER"]] = initial["fingerprint"]
        return runtime, obj, collection
    obj = ObjectState(
        name="Owned", type="EMPTY", library=None, data=None,
        location=[0.0, 0.0, 0.0], rotation_euler=[0.0, 0.0, 0.0],
        scale=[1.0, 1.0, 1.0], rotation_mode="XYZ",
        rotation_quaternion=[1.0, 0.0, 0.0, 0.0],
        rotation_axis_angle=[0.0, 0.0, 1.0, 0.0],
        delta_location=[0.0, 0.0, 0.0],
        delta_rotation_euler=[0.0, 0.0, 0.0],
        delta_rotation_quaternion=[1.0, 0.0, 0.0, 0.0],
        delta_scale=[1.0, 1.0, 1.0],
        parent=None, parent_type="OBJECT", parent_bone="",
        matrix_parent_inverse=[[1.0 if x == y else 0.0 for x in range(4)]
                               for y in range(4)],
        users_collection=[SimpleNamespace(name="Owned")],
        hide_render=False, hide_viewport=False, modifiers=[], constraints=[],
        animation_data=None,
    )
    scene = SimpleNamespace(
        unit_settings=SimpleNamespace(scale_length=1.0),
        render=SimpleNamespace(fps=24, fps_base=1.0), frame_current=1,
    )
    runtime = namespace["AuthoringRuntime"](
        SimpleNamespace(context=SimpleNamespace(scene=scene)),
        None, SimpleNamespace(changed=lambda: None),
    )
    marker = {}
    runtime._objects = lambda island: [obj]
    runtime.island = lambda island: marker
    runtime._closed = lambda island: (marker, [obj])
    initial = runtime.snapshot("owned")
    marker[namespace["MARKER"]] = initial["fingerprint"]
    return runtime, obj, marker


class TransformRevisionTests(unittest.TestCase):
    def test_owner_transform_state_changes_refuse_old_native_begin(self):
        mutations = (
            ("rotation_mode", "ZYX"),
            ("rotation_mode", "QUATERNION"),
            ("rotation_mode", "AXIS_ANGLE"),
            ("rotation_quaternion", [0.0, 0.0, 0.0, 1.0]),
            ("rotation_axis_angle", [1.0, 0.0, 0.0, 1.0]),
            ("delta_location", [4.0, 0.0, 0.0]),
            ("delta_rotation_euler", [0.0, 0.0, 0.4]),
            ("delta_rotation_quaternion", [0.0, 1.0, 0.0, 0.0]),
            ("delta_scale", [2.0, 1.0, 1.0]),
        )
        for source in SOURCES:
            namespace = load_runtime(source)
            for field, value in mutations:
                with self.subTest(source=source.parent.name, field=field, value=value):
                    runtime, obj, marker = fixture(namespace)
                    before = runtime.snapshot("owned")
                    setattr(obj, field, copy.deepcopy(value))
                    owner_state = transform_values(obj)
                    observed = runtime.snapshot("owned")
                    self.assertNotEqual(before["fingerprint"], observed["fingerprint"])
                    self.assertTrue(observed["drift"])
                    with self.assertRaises(CommandError) as caught:
                        runtime.begin(before["fingerprint"], "owned")
                    self.assertEqual(caught.exception.code, "StaleReference")
                    self.assertEqual(transform_values(obj), owner_state)
                    self.assertEqual(marker[namespace["MARKER"]], before["fingerprint"])

    def test_quaternion_and_axis_angle_edits_remain_visible_after_mode_selection(self):
        for source in SOURCES:
            namespace = load_runtime(source)
            for mode, field, value in (
                ("QUATERNION", "rotation_quaternion", [0.0, 0.0, 0.0, 1.0]),
                ("AXIS_ANGLE", "rotation_axis_angle", [1.0, 0.0, 0.0, 1.0]),
            ):
                with self.subTest(source=source.parent.name, mode=mode):
                    runtime, obj, marker = fixture(namespace)
                    obj.rotation_mode = mode
                    base = runtime.snapshot("owned")
                    marker[namespace["MARKER"]] = base["fingerprint"]
                    setattr(obj, field, value)
                    with self.assertRaises(CommandError) as caught:
                        runtime.begin(base["fingerprint"], "owned")
                    self.assertEqual(caught.exception.code, "StaleReference")
                    self.assertEqual(list(getattr(obj, field)), value)

    def test_unchanged_default_xyz_keeps_existing_library_fingerprint(self):
        for source in SOURCES:
            with self.subTest(source=source.parent.name):
                namespace = load_runtime(source)
                runtime, obj, _ = fixture(namespace)
                row = runtime._row(obj)
                self.assertNotIn("transform_state", row)
                baseline = namespace["digest"](namespace["source_projection_value"]({
                    "schema": "blender-source-projection-v4", "items": [row],
                    "scene_units": 1.0, "fps": 24, "fps_base": 1.0, "frame": 1,
                }))
                # Captured on the unpatched parent, not derived from the new helper.
                self.assertEqual(
                    baseline,
                    "53da7a84ea1cae7f7c81bef6313f3523f265fd766060f80d9240dbb2bcaa01f1",
                )
                observed = runtime.begin(baseline, "owned")
                self.assertEqual(observed["fingerprint"], baseline)
                self.assertFalse(observed["drift"])

    def test_whole_scene_preservation_sees_extended_transform_state(self):
        for source in SOURCES:
            with self.subTest(source=source.parent.name):
                namespace = load_runtime(source)
                runtime, obj, _ = fixture(namespace)
                before = runtime.snapshot()
                obj.delta_location = [0.0, 5.0, 0.0]
                after = runtime.snapshot()
                self.assertNotEqual(before["items"], after["items"])
                self.assertNotEqual(before["fingerprint"], after["fingerprint"])

    def test_fresh_reconciled_nondefault_state_remains_readable(self):
        for source in SOURCES:
            with self.subTest(source=source.parent.name):
                namespace = load_runtime(source)
                runtime, obj, _ = fixture(namespace)
                obj.rotation_mode = "QUATERNION"
                obj.rotation_quaternion = [0.0, 0.0, 0.0, 1.0]
                current = runtime.snapshot("owned")
                observed = runtime.begin(current["fingerprint"], "owned", allow_drift=True)
                self.assertEqual(observed["fingerprint"], current["fingerprint"])
                self.assertTrue(observed["drift"])


if __name__ == "__main__":
    if "--native" in sys.argv:
        import bpy
        NATIVE_BPY = bpy
        print(json.dumps({"native_blender": bpy.app.version_string,
                          "sources": [str(path.relative_to(ROOT)) for path in SOURCES]}),
              flush=True)
    unittest.main(argv=[sys.argv[0], "-v"])
