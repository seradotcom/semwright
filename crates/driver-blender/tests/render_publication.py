"""Render publication regression: pure fixtures by default; real Blender with --native."""
import base64
import importlib
import json
from pathlib import Path
import sys
import tempfile
from types import ModuleType, SimpleNamespace as N
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[3]
SOURCES = (
    ROOT / "adapters/blender/semwright_blender",
    ROOT / "crates/driver-host/src/windows_blender52_runtime",
)
PNG = base64.b64decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAusB9Y9Z83sAAAAASUVORK5CYII=")
NATIVE_BPY = None


def source_modules():
    for index, directory in enumerate(SOURCES):
        name = "render_publication_source_" + str(index)
        if name not in sys.modules:
            package = ModuleType(name)
            package.__path__ = [str(directory)]
            sys.modules[name] = package
        yield importlib.import_module(name + ".commands")


def fixture(module, root):
    calls = []
    if NATIVE_BPY is None:
        render = N(filepath="previous-path", image_settings=N(file_format="JPEG"),
                   use_multiview=False, use_compositing=True)
        scene = N(render=render, use_nodes=False, compositing_node_group=None)
        data = N()

        def writer(**kwargs):
            calls.append(render.filepath)
            Path(render.filepath).write_bytes(PNG)
            return {"FINISHED"}
    else:
        bpy = NATIVE_BPY
        scene = bpy.context.scene
        render = scene.render
        render.engine = "CYCLES"
        scene.cycles.device = "CPU"
        scene.cycles.samples = 1
        scene.cycles.use_denoising = False
        render.resolution_x = render.resolution_y = 32
        render.resolution_percentage = 100
        render.threads_mode = "FIXED"
        render.threads = 1
        render.use_multiview = False
        render.use_compositing = False
        render.filepath = "previous-path"
        render.image_settings.file_format = "JPEG"
        data = bpy.data

        def writer(**kwargs):
            calls.append(render.filepath)
            return bpy.ops.render.render(**kwargs)
    native = N(data=data, context=N(scene=scene), ops=N(render=N(render=writer)))
    return module.Commands(native, str(root)), native, calls


class RenderPublicationTests(unittest.TestCase):
    def each_source(self):
        for module in source_modules():
            with tempfile.TemporaryDirectory(prefix="semwright-render-contract-") as directory:
                root = Path(directory).resolve()
                commands, bpy, calls = fixture(module, root)
                yield module, root, commands, bpy, calls

    def assert_restored(self, bpy):
        self.assertEqual(bpy.context.scene.render.filepath, "previous-path")
        self.assertEqual(bpy.context.scene.render.image_settings.file_format, "JPEG")

    def test_new_only_publishes_one_png_and_restores_settings(self):
        for module, root, commands, bpy, calls in self.each_source():
            result = commands("blender.render", {"path": "preview.png", "overwrite": False})
            self.assertEqual(result, {"changed": True, "path": "preview.png", "format": "png"})
            self.assertEqual(len(calls), 1)
            self.assertNotEqual(Path(calls[0]), root / "preview.png")
            self.assertEqual(Path(calls[0]).parent, root)
            self.assertTrue((root / "preview.png").read_bytes().startswith(b"\x89PNG\r\n\x1a\n"))
            self.assertEqual({item.name for item in root.iterdir()}, {"preview.png"})
            self.assert_restored(bpy)

    def test_existing_and_racing_targets_are_preserved(self):
        for module, root, commands, bpy, calls in self.each_source():
            existing = root / "existing.png"
            existing.write_bytes(b"owner's original")
            with self.assertRaises(module.CommandError) as caught:
                commands("blender.render", {"path": existing.name, "overwrite": False})
            self.assertEqual(caught.exception.code, "Conflict")
            self.assertEqual(calls, [])
            self.assertEqual(existing.read_bytes(), b"owner's original")
            target = root / "racing.png"
            real_link = module.os.link

            def concurrent_publication(source, destination, *args, **kwargs):
                self.assertEqual(Path(destination), target)
                with target.open("xb") as output:
                    output.write(b"concurrent owner output")
                return real_link(source, destination, *args, **kwargs)

            with patch.object(module.os, "link", side_effect=concurrent_publication):
                with self.assertRaises(module.CommandError) as caught:
                    commands("blender.render", {"path": target.name, "overwrite": False})
            self.assertEqual(caught.exception.code, "Conflict")
            self.assertEqual(len(calls), 1)
            self.assertEqual(target.read_bytes(), b"concurrent owner output")
            self.assertEqual({item.name for item in root.iterdir()}, {"existing.png", "racing.png"})
            self.assert_restored(bpy)

    def test_failure_after_publication_preserves_final_output(self):
        for module, root, commands, bpy, calls in self.each_source():
            target = root / "published.png"
            path = commands.workspace.path

            def fail_readback(relative, suffix, existing=False):
                if existing:
                    self.assertTrue(target.read_bytes().startswith(b"\x89PNG\r\n\x1a\n"))
                    raise OSError("controlled post-publication readback failure")
                return path(relative, suffix, existing=existing)

            with patch.object(commands.workspace, "path", side_effect=fail_readback):
                with self.assertRaises(module.CommandError) as caught:
                    commands("blender.render", {"path": target.name, "overwrite": False})
            self.assertEqual(caught.exception.code, "BackendFailed")
            self.assertEqual(len(calls), 1)
            self.assertTrue(target.read_bytes().startswith(b"\x89PNG\r\n\x1a\n"))
            self.assertEqual({item.name for item in root.iterdir()}, {"published.png"})
            self.assert_restored(bpy)

    def test_legacy_omission_and_explicit_true_keep_prior_behavior(self):
        for module, root, commands, bpy, calls in self.each_source():
            for extra in ({}, {"overwrite": True}):
                with self.subTest(extra=extra):
                    target = root / "legacy.png"
                    target.write_bytes(b"legacy prior content")
                    result = commands("blender.render", {"path": target.name, **extra})
                    self.assertTrue(result["changed"])
                    self.assertEqual(target.read_bytes(), PNG)
                    self.assertEqual(Path(calls[-1]), target)
                    self.assert_restored(bpy)

    def test_overwrite_must_be_a_boolean(self):
        for module, root, commands, bpy, calls in self.each_source():
            for value in (None, "false", 0, 1, [], {}):
                with self.subTest(value=value), self.assertRaises(module.CommandError) as caught:
                    commands("blender.render", {"path": "invalid.png", "overwrite": value})
                self.assertEqual(caught.exception.code, "InvalidArgument")
            self.assertEqual(calls, [])
            self.assertEqual(list(root.iterdir()), [])

    def test_incomplete_native_render_never_publishes(self):
        for module, root, commands, bpy, calls in self.each_source():
            for failure in ("cancel", "invalid", "exception"):
                with self.subTest(failure=failure):
                    def writer(**kwargs):
                        calls.append(bpy.context.scene.render.filepath)
                        Path(calls[-1]).write_bytes(b"not a PNG")
                        if failure == "exception":
                            raise RuntimeError("controlled renderer error")
                        return {"CANCELLED"} if failure == "cancel" else {"FINISHED"}
                    bpy.ops.render.render = writer
                    with self.assertRaises(module.CommandError) as caught:
                        commands("blender.render", {"path": "failed.png", "overwrite": False})
                    self.assertEqual(caught.exception.code, "BackendFailed")
                    self.assertEqual(list(root.iterdir()), [])
                    self.assert_restored(bpy)

    def test_new_only_refuses_multiple_native_output_paths(self):
        for module, root, commands, bpy, calls in self.each_source():
            scene = bpy.context.scene
            for field, value in (("use_multiview", True), ("use_nodes", True),
                                 ("compositing_node_group", N(nodes=[]))):
                owner = scene.render if field == "use_multiview" else scene
                previous = getattr(owner, field)
                setattr(owner, field, value)
                with self.subTest(field=field), self.assertRaises(module.CommandError) as caught:
                    commands("blender.render", {"path": "multi.png", "overwrite": False})
                self.assertEqual(caught.exception.code, "Unsupported")
                setattr(owner, field, previous)
            self.assertEqual(calls, [])
            self.assertEqual(list(root.iterdir()), [])

    def test_canonical_schema_and_embedded_schema_agree(self):
        descriptors = json.loads((ROOT / "schemas/commands.json").read_text())
        schema = next(row for row in descriptors if row["name"] == "blender.render")["input_schema"]
        self.assertEqual(schema["properties"]["overwrite"]["type"], "boolean")
        self.assertIs(schema["properties"]["overwrite"]["default"], True)
        for module in source_modules():
            self.assertEqual(module.SCHEMAS["blender.render"], schema)


if __name__ == "__main__":
    if "--native" in sys.argv:
        import bpy
        NATIVE_BPY = bpy
        print(json.dumps({"native_blender": bpy.app.version_string,
                          "render_profile": "32x32 Cycles CPU, one sample",
                          "broker_host_admission": False}), flush=True)
        suite = unittest.TestSuite(RenderPublicationTests(name) for name in (
            "test_new_only_publishes_one_png_and_restores_settings",
            "test_existing_and_racing_targets_are_preserved",
            "test_failure_after_publication_preserves_final_output",
        ))
        result = unittest.TextTestRunner(verbosity=2).run(suite)
        raise SystemExit(0 if result.wasSuccessful() else 1)
    unittest.main(argv=[sys.argv[0], "-v"])
