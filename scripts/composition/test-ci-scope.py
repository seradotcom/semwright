#!/usr/bin/env python3
import importlib.util, unittest
from pathlib import Path
HERE=Path(__file__).resolve().parent
SPEC=importlib.util.spec_from_file_location("ci_scope",HERE/"ci-scope.py")
assert SPEC and SPEC.loader
scope=importlib.util.module_from_spec(SPEC); SPEC.loader.exec_module(scope)
class ScopeTests(unittest.TestCase):
    def test_media_time_only_hits_its_dependents(self):
        f=scope.classify(["crates/media-time/src/lib.rs"])
        self.assertTrue(f["contracts"]); self.assertTrue(f["motion"])
        self.assertTrue(f["fuzz_media"]); self.assertTrue(f["mutants_media"])
        self.assertFalse(f["mlt"]); self.assertFalse(f["figma"])
        self.assertFalse(f["fuzz_kernel"]); self.assertFalse(f["skills"])
    def test_av_contract_change_does_not_retest_motion_or_figma(self):
        f=scope.classify(["crates/av-composition/src/coordinator.rs"])
        self.assertTrue(f["contracts"]); self.assertTrue(f["fuzz_av"]); self.assertTrue(f["mutants_av"])
        self.assertFalse(f["motion"]); self.assertFalse(f["figma"]); self.assertFalse(f["mlt"])
    def test_skill_only_does_not_trigger_native_drivers(self):
        f=scope.classify(["skills/semwright-av-production/SKILL.md"])
        self.assertTrue(f["skills"])
        self.assertFalse(f["motion"]); self.assertFalse(f["figma"]); self.assertFalse(f["mlt"])
        self.assertFalse(f["contracts"])
    def test_motion_driver_only_is_narrow(self):
        f=scope.classify(["crates/driver-motion-canvas/src/renderer.rs"])
        self.assertTrue(f["motion"])
        self.assertFalse(f["figma"]); self.assertFalse(f["mlt"]); self.assertFalse(f["contracts"])
    def test_semantic_kernel_reaches_figma_motion_and_kernel_fuzz(self):
        f=scope.classify(["crates/semantic-composition/src/controller.rs"])
        self.assertTrue(f["contracts"]); self.assertTrue(f["figma"]); self.assertTrue(f["motion"])
        self.assertTrue(f["fuzz_kernel"]); self.assertTrue(f["mutants_kernel"])
    def test_certification_forces_every_area(self):
        f=scope.classify([],True)
        self.assertTrue(all(f.values()))
if __name__=="__main__": unittest.main()
