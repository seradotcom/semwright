"""Synthetic metadata regressions, not SDK/runtime acceptance."""
import copy
import json
import unittest
from pathlib import Path
from audit_contract import audit, validate_source_lock, validate_acceptance

ROOT = Path(__file__).resolve().parents[2]

class AuditTests(unittest.TestCase):
    def setUp(self):
        docs = ROOT / "docs/native-sdk"
        self.lock = json.loads((docs / "SOURCE_LOCK.json").read_text())
        self.matrix = json.loads((docs / "ACCEPTANCE_MATRIX.json").read_text())

    def test_complete_inventory(self):
        self.assertEqual(audit(ROOT)["members"], 121)

    def test_complete_acceptance(self):
        self.assertEqual(validate_acceptance(self.matrix), 30)

    def test_wrong_archive_bytes_rejected(self):
        self.lock["sources"][0]["sha256"] = "0" * 64
        with self.assertRaises(ValueError): validate_source_lock(self.lock)

    def test_duplicate_member_rejected(self):
        items = self.lock["sources"][0]["files"]
        items[1] = copy.deepcopy(items[0])
        with self.assertRaises(ValueError): validate_source_lock(self.lock)

    def test_traversal_member_rejected(self):
        self.lock["sources"][0]["files"][0]["path"] = "../outside"
        with self.assertRaises(ValueError): validate_source_lock(self.lock)

    def test_omitted_checksum_rejected(self):
        self.lock["sources"][0]["internal_hashes_verified"] -= 1
        with self.assertRaises(ValueError): validate_source_lock(self.lock)

    def test_license_not_inferred(self):
        self.lock["public_distribution_permitted"] = True
        with self.assertRaises(ValueError): validate_source_lock(self.lock)

    def test_zero_tests_cannot_pass(self):
        row = self.matrix["scenarios"][0]
        row.update(status="PASS", evidence=[{"ci_provider":"github-actions", "sha":"a"*40, "tests":0}])
        with self.assertRaises(ValueError): validate_acceptance(self.matrix)

    def test_circleci_not_final_certification(self):
        row = self.matrix["scenarios"][0]
        row.update(status="PASS", evidence=[{"ci_provider":"circleci", "sha":"a"*40, "tests":1}])
        with self.assertRaises(ValueError): validate_acceptance(self.matrix)

    def test_skips_cannot_certify(self):
        row = self.matrix["scenarios"][0]
        row.update(status="PASS", evidence=[{"ci_provider":"github-actions", "sha":"a"*40, "tests":1, "skipped":1}])
        with self.assertRaises(ValueError): validate_acceptance(self.matrix)

if __name__ == "__main__": unittest.main()
