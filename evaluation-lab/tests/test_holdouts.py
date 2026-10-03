"""Synthetic seal/reveal controls; these fixtures are exposed public tests."""
from copy import deepcopy
import json
from pathlib import Path
import sys
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT))
import holdouts


class HeldoutCommitmentTests(unittest.TestCase):
    def setUp(self):
        self.public = json.loads((ROOT/"tasks/public-dev.json").read_text())
        self.registry = holdouts.reserve(self.public,round_id="synthetic-control",seed=194)
        self.payload = {"registry":self.registry,"round_id":"synthetic-control",
                        "technical_product_sha":"1"*40,"nonce":"2"*64}

    def test_fresh_variations_preserve_all_families_phases_and_public_templates(self):
        before = deepcopy(self.public)
        heldout = holdouts.reserve(self.public,round_id="synthetic-control",seed=194)
        self.assertEqual(self.public,before)
        self.assertEqual({t["family"] for t in heldout["tasks"]},{t["family"] for t in self.public["tasks"]})
        for old,new in zip(self.public["tasks"],heldout["tasks"]):
            self.assertEqual(old["phases"],new["phases"])
            self.assertNotEqual(old["parameters"],new["parameters"])

    def test_deterministic_reservation_and_next_round_changes_identity(self):
        self.assertEqual(self.registry,holdouts.reserve(self.public,round_id="synthetic-control",seed=194))
        self.assertNotEqual(self.registry,holdouts.reserve(self.public,round_id="next-control",seed=195))

    def test_public_seal_contains_no_private_parameters(self):
        sealed = holdouts.seal(self.payload)
        self.assertNotIn("registry",sealed)
        self.assertNotIn("nonce",sealed)
        self.assertEqual(sealed["state"],"DRAFT_RESERVED_NOT_FINAL_EVALUATION_FREEZE")
        self.assertFalse(sealed["model_protocol_frozen"])

    def test_reveal_rejects_changed_parameters_and_wrong_product(self):
        sealed = holdouts.seal(self.payload)
        changed_parameters = deepcopy(self.payload)
        changed_parameters["registry"]["tasks"][0]["parameters"]["objective_count"] += 1
        changed_nonce = deepcopy(self.payload);changed_nonce["nonce"]="4"*64
        for changed in (changed_parameters,changed_nonce):
            with self.assertRaises(ValueError):
                holdouts.reveal(changed,sealed,consumed_rounds=[])
        changed = deepcopy(sealed);changed["technical_product_sha"]="3"*40
        with self.assertRaises(ValueError):
            holdouts.reveal(self.payload,changed,consumed_rounds=[])

    def test_reveal_consumes_round_and_refuses_reuse(self):
        sealed=holdouts.seal(self.payload)
        revealed=holdouts.reveal(self.payload,sealed,consumed_rounds=[])
        self.assertEqual(revealed["payload"],self.payload)
        with self.assertRaises(ValueError):
            holdouts.reveal(self.payload,sealed,consumed_rounds=[self.payload["round_id"]])


if __name__ == "__main__":
    unittest.main()
