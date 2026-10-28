"""Windows lexical/NTFS workspace checks; these are not native Blender certification."""
import importlib
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import types
import unittest
from unittest import mock

REPO = Path(__file__).resolve().parents[2]
PACKAGE = "_semwright_windows_path_contract"
namespace = types.ModuleType(PACKAGE)
namespace.__path__ = [str(REPO / "adapters" / "blender" / "semwright_blender")]
sys.modules[PACKAGE] = namespace
module = importlib.import_module(PACKAGE + ".commands")
Workspace, CommandError = module.Workspace, module.CommandError

class LexicalContract(unittest.TestCase):
    def test_windows_ambiguous_components_are_denied(self):
        for value in ("", ".", "..", "a\\b", "a:b", "CON.glb", "nul.blend", "PRN.txt",
                      "COM1.blend", "LPT9.glb", "COM\u00b9.blend", "CONIN$", "CONOUT$",
                      "NUL .blend", "tail.", "tail ", "a?b", "a\n.glb"):
            with self.subTest(value=value):
                self.assertTrue(Workspace._windows_component_unsafe(value))

    def test_portable_component_names_are_retained(self):
        for value in ("exports", "ARQ_01-v2.blend", "Costa de los Vientos.glb",
                      ".private", "conifer.glb", "compound.name.glb"):
            with self.subTest(value=value):
                self.assertFalse(Workspace._windows_component_unsafe(value))

    def test_reparse_attribute_is_not_confused_with_regular_file_mode(self):
        self.assertTrue(Workspace._reparse(types.SimpleNamespace(st_file_attributes=0x400)))
        self.assertFalse(Workspace._reparse(types.SimpleNamespace(st_file_attributes=0x20)))

@unittest.skipUnless(os.name == "nt", "requires actual Windows filesystem")
class WindowsWorkspaceContract(unittest.TestCase):
    def setUp(self):
        scratch = REPO / ".local" / "workspace-path-tests"
        scratch.mkdir(parents=True, exist_ok=True)
        self.temporary = tempfile.TemporaryDirectory(prefix="case-", dir=scratch)
        self.base = Path(self.temporary.name)
        self.root = self.base / "workspace"
        self.outside = self.base / "outside"
        self.root.mkdir()
        self.outside.mkdir()
        (self.outside / "sentinel.blend").write_bytes(b"owned test fixture")
        self.workspace = Workspace(str(self.root.resolve(strict=True)))

    def tearDown(self):
        self.temporary.cleanup()

    def denied(self, value, existing=False):
        with self.assertRaises(CommandError) as caught:
            self.workspace.path(value, ".blend", existing=existing)
        self.assertEqual(caught.exception.code, "PolicyDenied")

    def junction(self, path, target):
        result = subprocess.run(["cmd.exe", "/d", "/c", "mklink", "/J", str(path), str(target)],
                                capture_output=True, text=True, errors="replace", check=False)
        self.assertEqual(result.returncode, 0, "Junction fixture creation failed: " + result.stderr)

    def test_portable_relative_file_and_new_output(self):
        (self.root / "input.blend").write_bytes(b"fixture")
        self.assertEqual(self.workspace.path("input.blend", ".blend", existing=True), str(self.root / "input.blend"))
        self.assertEqual(self.workspace.path("new.blend", ".blend"), str(self.root / "new.blend"))

    def test_namespaces_ads_and_backslashes_rejected_before_access(self):
        for value in ("C:/outside.blend", "input.blend:secret", r"\\server\share\input.blend",
                      r"\\?\C:\input.blend", r"\\.\pipe\input.blend", r"a\..\input.blend",
                      "NUL.blend", "COM1.blend", "folder./input.blend", "folder /input.blend"):
            with self.subTest(value=value):
                self.denied(value)

    def test_root_namespaces_rejected_without_resolution(self):
        with mock.patch.object(Path, "resolve", side_effect=AssertionError("namespace was resolved")) as resolve:
            for root in (r"\\server\share\workspace", r"\\.\pipe\workspace", r"\\?\UNC\server\share\workspace"):
                with self.subTest(root=root), self.assertRaises(ValueError):
                    Workspace(root)
            resolve.assert_not_called()

    def test_volume_root_is_not_a_workspace(self):
        with self.assertRaises(ValueError):
            Workspace(self.root.anchor)

    def test_junction_parent_is_denied(self):
        link = self.root / "redirect"
        self.junction(link, self.outside)
        try:
            self.denied("redirect/sentinel.blend", existing=True)
        finally:
            os.rmdir(link)

    def test_existing_hardlinked_file_is_denied(self):
        os.link(self.outside / "sentinel.blend", self.root / "alias.blend")
        self.assertGreater((self.root / "alias.blend").stat().st_nlink, 1)
        self.denied("alias.blend", existing=True)

    def test_workspace_root_replaced_by_junction_is_denied(self):
        original = self.base / "original-workspace"
        self.root.rename(original)
        self.junction(self.root, self.outside)
        try:
            self.denied("sentinel.blend", existing=True)
        finally:
            os.rmdir(self.root)
            original.rename(self.root)

if __name__ == "__main__":
    unittest.main()
