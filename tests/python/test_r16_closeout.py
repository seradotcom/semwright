"""Small evidence-format checks, not a product security verdict."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("r16_validator", ROOT / "scripts/review/validate_r16.py")
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


def record():
    return {"review_target_sha": "a" * 40, "final_source_sha": "b" * 40,
            "r16_closed": False, "external_audit": False,
            "areas": [{"id": f"R16-{i:02}", "limitations": ["Not a full audit"],
                       "source_locations": ["source.rs:1-2"]} for i in range(1, 13)]}


class EvidenceTests(unittest.TestCase):
    def test_valid_minimal_record(self):
        self.assertEqual(MODULE.evidence_errors(record()), [])

    def test_short_sha_rejected(self):
        data = record()
        data["review_target_sha"] = "a" * 7
        self.assertTrue(MODULE.evidence_errors(data))

    def test_self_closure_rejected(self):
        data = record()
        data["r16_closed"] = True
        self.assertTrue(MODULE.evidence_errors(data))

    def test_external_audit_claim_rejected(self):
        data = record()
        data["external_audit"] = True
        self.assertTrue(MODULE.evidence_errors(data))

    def test_duplicate_area_rejected(self):
        data = record()
        data["areas"][-1] = data["areas"][0]
        self.assertTrue(MODULE.evidence_errors(data))

    def test_limitations_required(self):
        data = record()
        data["areas"][0]["limitations"] = []
        self.assertTrue(MODULE.evidence_errors(data))

    def test_unsupported_pass_rejected(self):
        data = record()
        data["executions"] = [{"result": "PASS", "source_sha": "a" * 40}]
        self.assertTrue(MODULE.evidence_errors(data))

    def test_skips_not_executions(self):
        data = record()
        data["executions"] = [{"result": "SKIPPED", "executed": True}]
        self.assertTrue(MODULE.evidence_errors(data))

    def test_duplicate_json_keys_rejected(self):
        with self.assertRaises(ValueError):
            json.loads('{"result":"FAIL","result":"PASS"}', object_pairs_hook=MODULE.duplicate_keys)

    def test_local_links_and_external_exclusion(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root / "exists.md").write_text("# Existing")
            (root / "README.md").write_text("[ok](exists.md) [bad](missing.md) [web](https://example.com)")
            errors = MODULE.links(root, "README.md")
            self.assertEqual(len(errors), 1)
            self.assertIn("missing.md", errors[0])


if __name__ == "__main__":
    unittest.main()
