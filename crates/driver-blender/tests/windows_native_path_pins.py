"""Closed native path pins: actual small Windows filesystem cases, no Blender launch."""
import ast
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

BOOTSTRAP = Path(__file__).resolve().parents[2] / "driver-host/src/windows_blender52_bootstrap.py"
tree = ast.parse(BOOTSTRAP.read_text(encoding="utf-8"))
helpers = ast.Module(body=[node for node in tree.body
                          if isinstance(node, (ast.ClassDef, ast.FunctionDef))
                          and node.name in {"_native_path_probe", "NativePathPins"}],
                     type_ignores=[])
namespace = {"Path": Path}
exec(compile(helpers, str(BOOTSTRAP), "exec"), namespace)
NativePathPins = namespace["NativePathPins"]
probe = namespace["_native_path_probe"]


@unittest.skipUnless(os.name == "nt", "Windows native filesystem required")
class NativePathPinsTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="sw-pin-")
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name).resolve(strict=True)
        self.records = []
        self.roots = {}
        for role in ("workspace", "native-bootstrap", "blender-runtime"):
            root = self.base / role
            root.mkdir()
            nt, identity, directory, _ = probe(root)
            self.assertTrue(directory)
            self.roots[role] = root
            self.records.append(dict(role=role, path=str(root), canonical=str(root), nt=nt,
                                     identity=list(identity)))
        self.workspace = self.roots["workspace"]
        self.pins = NativePathPins(self.records)

    def test_existing_nested_file_without_dos_canonicalization(self):
        folder = self.workspace / "Subdir"
        folder.mkdir()
        asset = folder / "asset.blend"
        asset.write_bytes(b"fixture")
        with patch.object(Path, "resolve", side_effect=PermissionError("DOS translation denied")):
            self.assertEqual(self.pins.resolve(self.workspace, "workspace"), self.workspace)
            self.assertEqual(self.pins.resolve(asset, "workspace"), asset)

    def test_refuses_outside_mount_and_cross_role(self):
        for path in (self.base, self.roots["native-bootstrap"], self.workspace / ".." / "native-bootstrap"):
            with self.subTest(path=path), self.assertRaises(PermissionError):
                self.pins.resolve(path, "workspace")

    def test_refuses_replaced_root(self):
        old = self.base / "previous-workspace"
        self.workspace.rename(old)
        self.workspace.mkdir()
        with self.assertRaises(PermissionError):
            self.pins.resolve(self.workspace, "workspace")

    def test_refuses_other_volume_or_physical_parent(self):
        asset = self.workspace / "file.png"
        asset.write_bytes(b"fixture")
        for fault in ("volume", "parent", "nested"):
            def altered(path):
                name, identity, directory, links = probe(path)
                if path == asset:
                    if fault == "volume":
                        identity = (identity[0] ^ 1, *identity[1:])
                    elif fault == "parent":
                        name = name.replace("\\workspace\\", "\\other\\")
                    else:
                        name = name.replace("\\file.png", "\\sub\\file.png")
                return name, identity, directory, links
            with self.subTest(fault=fault), self.assertRaises(PermissionError):
                NativePathPins(self.records, probe=altered).resolve(asset, "workspace")

    def test_refuses_multiple_hardlinks(self):
        asset = self.workspace / "file.png"
        asset.write_bytes(b"fixture")
        alias = self.workspace / "alias.png"
        os.link(asset, alias)
        with self.assertRaises(PermissionError):
            self.pins.resolve(asset, "workspace")

    def test_refuses_missing_and_unsafe_components(self):
        with self.assertRaises(FileNotFoundError):
            self.pins.resolve(self.workspace / "missing.blend", "workspace")
        for name in ("NUL.txt", "file:stream", "trailing.", "trailing "):
            with self.subTest(name=name), self.assertRaises(PermissionError):
                self.pins.resolve(self.workspace / name, "workspace")

    def test_refuses_canonical_alias_with_different_identity(self):
        records = [dict(item) for item in self.records]
        records[0]["canonical"] = str(self.roots["native-bootstrap"])
        with self.assertRaises(PermissionError):
            NativePathPins(records).resolve(self.workspace, "workspace")

    def test_refuses_invalid_pin_roles(self):
        bad = [dict(x) for x in self.records]
        bad[-1]["role"] = "workspace"
        with self.assertRaises(ValueError):
            NativePathPins(bad)

    def test_refuses_reparse_leaf(self):
        asset = self.workspace / "asset.blend"
        asset.write_bytes(b"fixture")
        link = self.workspace / "alias.blend"
        try:
            link.symlink_to(asset)
        except OSError as error:
            self.skipTest("runner cannot create disposable symlink: " + str(error.winerror))
        with self.assertRaises(PermissionError):
            self.pins.resolve(link, "workspace")


if __name__ == "__main__":
    unittest.main(verbosity=2)
