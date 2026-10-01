import importlib.util
import pathlib
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location(
    "ci_affected_areas", ROOT / "scripts/dev/ci-affected-areas.py"
)
MODULE = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(MODULE)


class CiAffectedAreasTests(unittest.TestCase):
    def test_mlt_change_is_narrow(self):
        areas = MODULE.classify(["crates/driver-mlt-video/src/main.rs"])
        self.assertTrue(areas["mlt"])
        self.assertTrue(areas["native_kicad_mlt"])
        self.assertFalse(areas["native_blender"])
        self.assertFalse(areas["platform_macos"])
        self.assertFalse(areas["core_rust"])

    def test_driver_host_change_reaches_host_consumers(self):
        areas = MODULE.classify(["crates/driver-host/src/lib.rs"])
        self.assertTrue(areas["core_rust"])
        self.assertTrue(areas["runtime_tools"])
        self.assertTrue(areas["native_driver_conformance"])
        self.assertTrue(areas["native_blender"])
        self.assertTrue(areas["native_figma"])
        self.assertTrue(areas["native_kicad_mlt"])
        self.assertTrue(areas["platform_windows"])
        self.assertTrue(areas["platform_macos"])

    def test_lockfile_change_runs_dependency_and_core_gates(self):
        areas = MODULE.classify(["Cargo.lock"])
        self.assertTrue(areas["core_rust"])
        self.assertTrue(areas["security_dependencies"])
        self.assertFalse(areas["security_fuzz"])

    def test_kicad_integration_path_is_native_scoped(self):
        areas = MODULE.classify(["integrations/kicad-driver/driver/src/main.rs"])
        self.assertTrue(areas["native_kicad_mlt"])
        self.assertFalse(areas["native_blender"])

    def test_full_forces_every_area(self):
        areas = MODULE.classify([], full=True)
        self.assertTrue(all(areas.values()))


if __name__ == "__main__":
    unittest.main()
