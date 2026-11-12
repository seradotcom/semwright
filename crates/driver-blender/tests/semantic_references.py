"""Small reference-lifetime regression for canonical and Host-embedded runtimes.

No Blender process or persistence is certified by these pure tests.
"""
import importlib.util
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace


ROOT = Path(__file__).resolve().parents[3]
MODULES = {}
for label, relative in (
    ("canonical", "crates/driver-blender/src/semantic.py"),
    ("windows_embedded", "crates/driver-host/src/windows_blender52_runtime/semantic.py"),
):
    spec = importlib.util.spec_from_file_location("semantic_refs_" + label, ROOT / relative)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    MODULES[label] = module


class NoDataLookup:
    def __getattr__(self, name):
        raise AssertionError("stale reference reached native data lookup: " + name)


class ReferenceLifetimeTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.workspace = str(Path(self.directory.name).resolve())

    def assert_stale(self, module, store, reference):
        with self.assertRaises(module.SemanticError) as caught:
            store._resolve(reference)
        self.assertEqual(caught.exception.code, "StaleReference")

    def test_fresh_store_rejects_same_name_and_generation_before_lookup(self):
        for label, module in MODULES.items():
            with self.subTest(runtime=label):
                first = module.SemanticStore(object(), self.workspace)
                fresh = module.SemanticStore(SimpleNamespace(data=NoDataLookup()), self.workspace)
                reference = first._ref("objects", "Cube")
                self.assertEqual(first.generation, fresh.generation)
                self.assert_stale(module, fresh, reference)

    def test_fresh_reference_resolves_current_object_and_nested_path(self):
        for label, module in MODULES.items():
            with self.subTest(runtime=label):
                vertex = object()
                mesh = SimpleNamespace(
                    vertices=[vertex],
                    bl_rna=SimpleNamespace(properties={"vertices": SimpleNamespace(type="COLLECTION")}),
                )
                cube = SimpleNamespace(
                    data=mesh,
                    bl_rna=SimpleNamespace(properties={"data": SimpleNamespace(type="POINTER")}),
                )
                store = module.SemanticStore(SimpleNamespace(data=SimpleNamespace(objects={"Cube": cube})), self.workspace)
                self.assertIs(store._resolve(store._ref("objects", "Cube"))[3], cube)
                reference = store._ref("objects", "Cube", [["p", "data"], ["c", "vertices", 0]])
                self.assertIs(store._resolve(reference)[3], vertex)

    def test_normal_generation_change_invalidates_old_reference(self):
        for label, module in MODULES.items():
            with self.subTest(runtime=label):
                store = module.SemanticStore(SimpleNamespace(data=NoDataLookup()), self.workspace)
                reference = store._ref("objects", "Cube")
                store.changed()
                self.assert_stale(module, store, reference)

    def test_counter_wrap_cannot_revive_generation_one_reference(self):
        for label, module in MODULES.items():
            with self.subTest(runtime=label):
                store = module.SemanticStore(SimpleNamespace(data=NoDataLookup()), self.workspace)
                reference = store._ref("objects", "Cube")
                store.generation = 2_147_483_647
                store.changed()
                self.assertEqual(store.generation, 1)
                self.assert_stale(module, store, reference)

    def test_unscoped_legacy_reference_requires_refresh(self):
        for label, module in MODULES.items():
            with self.subTest(runtime=label):
                store = module.SemanticStore(SimpleNamespace(data=NoDataLookup()), self.workspace)
                self.assert_stale(module, store, "blender-rna/v1/objects/Q3ViZQ@1")

    def test_wrong_scope_nested_reference_cannot_reach_native_data(self):
        for label, module in MODULES.items():
            with self.subTest(runtime=label):
                first = module.SemanticStore(object(), self.workspace)
                fresh = module.SemanticStore(SimpleNamespace(data=NoDataLookup()), self.workspace)
                reference = first._ref("objects", "Cube", [["p", "data"]])
                self.assert_stale(module, fresh, reference)


if __name__ == "__main__":
    unittest.main(verbosity=2)
