import json
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2] / "verification/pre-r16"


class PreR16EvidenceTests(unittest.TestCase):
    def test_original_heads_have_unique_explicit_dispositions(self):
        original = json.loads((ROOT / "inventory/branch-tree-comparison.json").read_text())
        ledger = json.loads((ROOT / "inventory/branch-dispositions-v2.json").read_text())
        heads = ledger["heads"]
        self.assertEqual({row["sha"] for row in original}, {row["sha"] for row in heads})
        self.assertEqual(len(heads), len({row["sha"] for row in heads}))
        self.assertEqual(ledger["count"], len(heads))
        self.assertEqual(ledger["unassigned"], 0)
        for row in heads:
            self.assertIn(row["classification"], set("ABCDEF"))
            self.assertTrue(row["reason"])
            self.assertTrue(row["evidence_paths"])
            self.assertFalse(row["delete_authorized"])

    def test_delegation_does_not_become_equivalence(self):
        ledger = json.loads((ROOT / "inventory/branch-dispositions-v2.json").read_text())
        delegated = [row for row in ledger["heads"] if row["disposition"] == "DELEGATED_APPLICATION_OWNER_REVIEW"]
        self.assertEqual(len(delegated), ledger["delegated_application_heads"])
        self.assertTrue(all(row["classification"] == "F" for row in delegated))
        self.assertFalse(ledger["cleanup_performed"])

    def test_pending_owners_cannot_produce_global_ready(self):
        audit = json.loads((ROOT / "pre-r16-audit.json").read_text())
        self.assertEqual(audit["r16_state"], "OPEN")
        self.assertFalse(audit["independent_review_performed"])
        if audit["must_land_prs"] or audit["must_resolve_owner_lanes"]:
            self.assertFalse(audit["r16_ready"])
            self.assertIsNone(audit["candidate_sha"])
        self.assertFalse(audit["ci"]["queued_is_pass"])
        self.assertFalse(audit["ci"]["skipped_is_pass"])
        self.assertEqual(len(audit["security_precheck"]), 12)
        self.assertEqual(len({row["area"] for row in audit["security_precheck"]}), 12)

    def test_recorded_prs_are_classified_without_claiming_live_refresh(self):
        state = json.loads((ROOT / "inventory/continuation-state.json").read_text())
        rows = state["prs"]
        self.assertEqual(len(rows), len({row["number"] for row in rows}))
        for row in rows:
            self.assertIn(row["classification"], set("ABCDEF"))
            self.assertTrue(row["owner"])
            self.assertTrue(row["reason"])
        audit = json.loads((ROOT / "pre-r16-audit.json").read_text())
        self.assertEqual(audit["open_prs"]["count_at_observation"], len(rows))


if __name__ == "__main__":
    unittest.main()
