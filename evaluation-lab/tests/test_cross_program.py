"""Control native producer/consumer revision registration, not final performance."""
import copy
import json
from pathlib import Path
import sys
import unittest

ROOT=Path(__file__).resolve().parents[1];sys.path.insert(0,str(ROOT/"native"))
from cross_program import stages


class CrossProgramTests(unittest.TestCase):
    def setUp(self):
        self.tasks=[t for t in json.loads((ROOT/"tasks/public-dev.json").read_text())["tasks"] if t["family"]=="cross_app"]

    def test_both_assets_have_six_independent_specs_without_modifying_registry(self):
        before=copy.deepcopy(self.tasks)
        for task in self.tasks:
            program=stages(task);self.assertEqual(len(program),6)
            program[0]["asset"]["color"][0]=0
            self.assertNotEqual(program[0]["asset"]["color"],program[1]["asset"]["color"])
        self.assertEqual(before,self.tasks)

    def test_native_replacement_preserves_the_revised_gameplay_rule(self):
        for task in self.tasks:
            program=stages(task);before,after=program[2:4]
            self.assertEqual(before["game"]["objective_count"],after["game"]["objective_count"])
            self.assertNotEqual(before["asset"]["width"],after["asset"]["width"])
            self.assertEqual(after["asset"]["width"],float(task["parameters"]["replacement_scale"]))

    def test_consumer_requires_the_actual_export_width_and_animation(self):
        for task in self.tasks:
            for item in stages(task):
                self.assertEqual(item["game"]["asset_source"],"res://asset.glb")
                self.assertEqual(item["game"]["asset_width"],item["asset"]["width"])
                self.assertEqual(item["game"]["asset_segments"],item["asset"]["segments"])
                self.assertGreater(item["game"]["asset_animation_seconds"],0)

    def test_unbounded_or_boolean_replacement_scale_is_rejected(self):
        for value in (True,0,100,"2"):
            task=copy.deepcopy(self.tasks[0]);task["parameters"]["replacement_scale"]=value
            with self.assertRaises(ValueError):stages(task)


if __name__=="__main__":
    unittest.main()
