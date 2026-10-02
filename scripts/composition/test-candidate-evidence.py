#!/usr/bin/env python3
import importlib.util,unittest
from pathlib import Path
HERE=Path(__file__).resolve().parent
SPEC=importlib.util.spec_from_file_location("candidate_evidence",HERE/"candidate-evidence.py")
assert SPEC and SPEC.loader
mod=importlib.util.module_from_spec(SPEC);SPEC.loader.exec_module(mod)
def base():
    return {
      "schema_version":1,"candidate_sha":"a"*40,"a_sha":"b"*40,"b_sha":"c"*40,
      "c0_sha":"26602e4b25929be869d69ef28fef4dd9713180d7",
      "combined_candidate":False,"b_audio_ready_for_integration":False,
      "ready_for_demo_production":False,"ready_for":[],
      "not_ready_for":["audio handoff pending"],
      "figma_required_for_workflow":True,"r16_closed":False,"promotional_video_created":False,
      "required_gates":{key:"PENDING" for key in mod.REQUIRED},"workflow_evidence":[]
    }
class Tests(unittest.TestCase):
    def test_pending_is_valid_but_not_ready(self):
        out=mod.validate(base());self.assertFalse(out["ready_for_demo_production"])
    def test_ready_cannot_mix_shas(self):
        v=base();v.update(combined_candidate=True,b_audio_ready_for_integration=True,ready_for_demo_production=True,
            ready_for=["verified-rendered-av-v1"],not_ready_for=[])
        v["required_gates"]={key:"PASS" for key in mod.REQUIRED}
        v["workflow_evidence"]=[{"workflow":"combined","run_id":1,"job_ids":[2],"tested_sha":"d"*40,
          "status":"PASS","artifacts":[],"limitations":[]}]
        with self.assertRaises(mod.Invalid):mod.validate(v)
    def test_ready_requires_b_and_all_gates(self):
        v=base();v.update(combined_candidate=True,ready_for_demo_production=True,
            ready_for=["verified-rendered-av-v1"],not_ready_for=[])
        v["required_gates"]={key:"PASS" for key in mod.REQUIRED}
        v["workflow_evidence"]=[{"workflow":"combined","run_id":1,"job_ids":[2],"tested_sha":"a"*40,
          "status":"PASS","artifacts":[],"limitations":[]}]
        with self.assertRaises(mod.Invalid):mod.validate(v)
    def test_non_figma_workflow_may_mark_figma_not_applicable(self):
        v=base();v.update(combined_candidate=True,b_audio_ready_for_integration=True,
            ready_for_demo_production=True,ready_for=["verified-rendered-av-v1"],
            not_ready_for=[],figma_required_for_workflow=False)
        v["required_gates"]={key:"PASS" for key in mod.REQUIRED}
        v["required_gates"]["figma_regression"]="NOT_APPLICABLE"
        v["workflow_evidence"]=[{"workflow":"combined","run_id":1,"job_ids":[2],"tested_sha":"a"*40,
          "status":"PASS","artifacts":[],"limitations":[]}]
        out=mod.validate(v)
        self.assertTrue(out["ready_for_demo_production"])
        self.assertTrue(out["all_required_gates_pass"])
    def test_figma_required_workflow_cannot_mark_figma_not_applicable(self):
        v=base();v.update(combined_candidate=True,b_audio_ready_for_integration=True,
            ready_for_demo_production=True,ready_for=["verified-rendered-av-v1"],not_ready_for=[])
        v["required_gates"]={key:"PASS" for key in mod.REQUIRED}
        v["required_gates"]["figma_regression"]="NOT_APPLICABLE"
        v["workflow_evidence"]=[{"workflow":"combined","run_id":1,"job_ids":[2],"tested_sha":"a"*40,
          "status":"PASS","artifacts":[],"limitations":[]}]
        with self.assertRaises(mod.Invalid):mod.validate(v)
if __name__=="__main__":unittest.main()
