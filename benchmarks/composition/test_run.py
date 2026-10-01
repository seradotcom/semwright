import copy
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("composition_benchmark", HERE / "run.py")
assert SPEC and SPEC.loader
bench = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(bench)

def template():
    return json.loads((HERE / "evidence.example.json").read_text())

def complete_pass():
    data = template()
    data["tested_sha"] = "a" * 40
    data["input_digest"] = "b" * 64
    data["output_profile_digest"] = "c" * 64
    task = bench.task(data["task_id"])
    for name in bench.ARMS:
        arm = data["arms"][name]
        arm.update({
            "status": "PASS",
            "setup_ms": 10,
            "execution_ms": 20 if name == "low_level" else 12,
            "calls": 5 if name == "low_level" else 2,
            "retries": 0,
            "interventions": 0,
            "native_editable": True,
            "verification": "PASS",
            "checks": {check: "PASS" for check in task["checks"]},
        })
    return data

class BenchmarkHarnessTests(unittest.TestCase):
    def test_complete_evidence_validates_without_winner(self):
        data = complete_pass()
        result = bench.validate(data)
        self.assertTrue(result["valid"])
        self.assertIsNone(result["winner"])

    def test_pass_cannot_hide_unknown_required_check(self):
        data = complete_pass()
        data["arms"]["high_level"]["checks"]["fresh_verification"] = "UNKNOWN"
        with self.assertRaises(bench.InvalidEvidence):
            bench.validate(data)

    def test_native_editability_is_required_for_editable_task(self):
        data = complete_pass()
        data["arms"]["low_level"]["native_editable"] = False
        with self.assertRaises(bench.InvalidEvidence):
            bench.validate(data)

    def test_blocked_arm_is_preserved_not_promoted(self):
        data = complete_pass()
        arm = data["arms"]["low_level"]
        arm["status"] = "BLOCKED"
        arm["verification"] = "NOT_RUN"
        result = bench.summarize(data)
        self.assertEqual(result["arms"]["low_level"]["status"], "BLOCKED")
        self.assertIsNone(result["interpretation"]["winner"])

    def test_summary_contains_raw_delta_not_score(self):
        result = bench.summarize(complete_pass())
        self.assertEqual(result["metrics"]["calls"]["high_minus_low"], -3)
        self.assertIsNone(result["interpretation"]["scores"])

if __name__ == "__main__":
    unittest.main()
