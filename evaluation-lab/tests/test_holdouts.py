"""Synthetic seal/reveal controls; these fixtures are exposed public tests."""
from copy import deepcopy
import json
from pathlib import Path
import sys
import tempfile
import concurrent.futures
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

    def test_durable_ledger_refuses_second_reveal_to_new_destination(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);ledger=root/"ledger"
            holdouts.reveal_with_ledger(self.payload,holdouts.seal(self.payload),ledger_directory=ledger,public_reveal=root/"first.json")
            with self.assertRaisesRegex(ValueError,"cannot be reused"):
                holdouts.reveal_with_ledger(self.payload,holdouts.seal(self.payload),ledger_directory=ledger,public_reveal=root/"second.json")
            self.assertFalse((root/"second.json").exists())
            value=json.loads(next(ledger.glob("*.json")).read_text())
            self.assertNotIn("payload",value)
            self.assertNotIn("registry",value)

    def test_failed_publication_preserves_user_file_and_burns_round(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);destination=root/"existing.json";destination.write_text("valuable existing evidence")
            with self.assertRaises(FileExistsError):
                holdouts.reveal_with_ledger(self.payload,holdouts.seal(self.payload),ledger_directory=root/"ledger",public_reveal=destination)
            self.assertEqual(destination.read_text(),"valuable existing evidence")
            with self.assertRaisesRegex(ValueError,"cannot be reused"):
                holdouts.reveal_with_ledger(self.payload,holdouts.seal(self.payload),ledger_directory=root/"ledger",public_reveal=root/"retry.json")

    def test_invalid_commitment_does_not_consume_valid_round(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);sealed=holdouts.seal(self.payload);sealed["commitment_sha256"]="0"*64
            with self.assertRaises(ValueError):
                holdouts.reveal_with_ledger(self.payload,sealed,ledger_directory=root/"ledger",public_reveal=root/"bad.json")
            self.assertFalse((root/"ledger").exists())

    def test_concurrent_reveal_has_one_publication_and_one_consumed_lease(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            def reveal(index):
                try:
                    holdouts.reveal_with_ledger(self.payload,holdouts.seal(self.payload),ledger_directory=root/"ledger",public_reveal=root/(str(index)+".json"))
                    return "published"
                except ValueError: return "refused"
            with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
                results=list(pool.map(reveal,(1,2)))
            self.assertEqual(sorted(results),["published","refused"])
            self.assertEqual(len(list(root.glob("*.json"))),1)
            self.assertEqual(len(list((root/"ledger").glob("*.json"))),1)

    def test_insecure_or_symlink_ledger_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);ledger=root/"unsafe";ledger.mkdir(mode=0o755)
            (root/"link").symlink_to(ledger,target_is_directory=True)
            for path in (ledger,root/"link"):
                with self.assertRaises(ValueError):
                    holdouts.reveal_with_ledger(self.payload,holdouts.seal(self.payload),ledger_directory=path,public_reveal=root/"bad.json")


if __name__ == "__main__":
    unittest.main()
