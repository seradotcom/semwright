"""Synthetic harness controls only; no model or native productivity results."""
import copy
import json
import tempfile
import unittest
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
import evaluate as lab


def fixtures():
    registry = json.loads((ROOT / "tasks/public-dev.json").read_text())
    # This is a unit-test identity, never a real heldout manifest or evaluation.
    registry["split"] = "heldout"
    freeze = {
        "schema_version": 1, "technical_gate": "PASS", "source_sha": "1"*40,
        "suite_sha": "2"*40, "task_digest": lab.digest(registry),
        "baseline_helpers_digest": "3"*64, "model_config_digest": "4"*64,
        "runtime_manifest_digest": "5"*64, "skills_manifest_digest": "6"*64,
        "model_access_authorized": True, "model_identity": "synthetic-unit-model",
        "arms": list(lab.ARMS), "studies": list(lab.STUDIES), "seeds": [17],
        "failure_propagation": "remaining_phases_failed",
        "budget": {"max_model_tokens": 12000, "max_tool_calls": 100,
                   "max_attempt_wall_ms": 1200000},
    }
    task = registry["tasks"][0]
    attempt = {"schema_version": 1, "execution_kind": "MODEL_AGENT",
               "source_sha": freeze["source_sha"], "suite_sha": freeze["suite_sha"],
               "task_digest": freeze["task_digest"], "model_config_digest": freeze["model_config_digest"],
               "model_identity": freeze["model_identity"], "task_id": task["id"],
               "study": "S", "seed": 17, "arm": "semantic", "outcome": "COMPLETED",
               "metrics": {k: {"value": None, "reason": "not observed in synthetic control"}
                           for k in lab.MEASUREMENTS},
               "phases": [{"name": phase, "outcome": "COMPLETED",
                           "checks": [{"name": name, "native": True, "outcome": "PASS",
                                       "evidence_sha256": "7"*64}
                                      for name in task["native_oracles"]]} for phase in lab.PHASES],
               "actions": [{"origin": "broker", "actor": "model", "authoring_mutation": True}]}
    return registry, freeze, attempt


class HarnessControls(unittest.TestCase):
    def setUp(self):
        self.registry, self.freeze, self.attempt = fixtures()

    def validate(self):
        return lab.validate_attempt(self.attempt, self.freeze, self.registry)

    def rejected(self):
        with self.assertRaises(ValueError):
            self.validate()

    def test_five_families_have_distinct_instances_and_revisions(self):
        self.assertEqual(len(lab.validate_registry(self.registry)["tasks"]), 10)

    def test_schedule_is_deterministic_paired_and_complete(self):
        rows = lab.schedule(self.freeze, self.registry)
        self.assertEqual(rows, lab.schedule(copy.deepcopy(self.freeze), self.registry))
        self.assertEqual(len(rows), 60)
        self.assertGreater(len({tuple(r["arm"] for r in rows[i:i+3])
                               for i in range(0, len(rows), 3)}), 1)

    def test_duplicate_tasks_rejected(self):
        self.registry["tasks"][1]["id"] = self.registry["tasks"][0]["id"]
        with self.assertRaises(ValueError):
            lab.validate_registry(self.registry)

    def test_relabelled_duplicate_parameters_rejected(self):
        self.registry["tasks"][1]["parameters"] = self.registry["tasks"][0]["parameters"]
        with self.assertRaises(ValueError):
            lab.validate_registry(self.registry)

    def test_technical_block_cannot_freeze(self):
        self.freeze["technical_gate"] = "BLOCKED"
        self.rejected()

    def test_public_tasks_cannot_be_final_holdouts(self):
        self.registry["split"] = "public_dev"
        self.freeze["task_digest"] = lab.digest(self.registry)
        self.rejected()

    def test_missing_authorized_model_cannot_freeze(self):
        self.freeze["model_access_authorized"] = False
        self.rejected()

    def test_deterministic_script_is_not_model_benchmark(self):
        self.attempt["execution_kind"] = "DETERMINISTIC_SCRIPT"
        self.rejected()

    def test_stale_binary_source_rejected(self):
        self.attempt["source_sha"] = "8"*40
        self.rejected()

    def test_changed_model_configuration_rejected(self):
        self.attempt["model_config_digest"] = "8"*64
        self.rejected()

    def test_native_positive_control(self):
        self.assertEqual(self.validate()["outcome"], "COMPLETED")

    def test_missing_native_oracle_rejected(self):
        self.attempt["phases"][0]["checks"].pop()
        self.rejected()

    def test_duplicate_oracle_rejected(self):
        self.attempt["phases"][0]["checks"].append(self.attempt["phases"][0]["checks"][0])
        self.rejected()

    def test_fake_check_cannot_certify_native_completion(self):
        self.attempt["phases"][0]["checks"][0]["native"] = False
        self.rejected()

    def test_unknown_check_does_not_become_pass(self):
        self.attempt["phases"][0]["checks"][0]["outcome"] = "UNKNOWN"
        self.rejected()

    def test_missing_revision_rejected(self):
        self.attempt["phases"].pop()
        self.rejected()

    def test_creation_failure_cannot_inherit_successful_base(self):
        self.attempt["outcome"] = "FAILED"
        self.attempt["phases"][0]["outcome"] = "FAILED"
        self.rejected()

    def test_unknown_costs_preserved(self):
        self.assertIsNone(self.validate()["metrics"]["input_tokens"]["value"])

    def test_missing_cost_reason_rejected(self):
        self.attempt["metrics"]["input_tokens"]["reason"] = None
        self.rejected()

    def test_boolean_or_negative_cost_is_not_measured_integer(self):
        for value in (True, -1, 1.5):
            self.attempt["metrics"]["user_wall_ms"] = {"value": value, "reason": None}
            self.rejected()

    def test_outside_semantic_mutation_is_route_violation(self):
        self.attempt["actions"][0]["origin"] = "model_file_write"
        self.rejected()
        self.attempt["outcome"] = "ROUTE_VIOLATION"
        self.assertEqual(self.validate()["outcome"], "ROUTE_VIOLATION")

    def test_direct_arm_may_use_competent_code(self):
        self.attempt["arm"] = "direct"
        self.attempt["actions"][0]["origin"] = "model_file_write"
        self.assertEqual(self.validate()["outcome"], "COMPLETED")

    def test_provisioning_cannot_author_hidden_game_behavior(self):
        self.attempt["actions"][0] = {"origin": "declared_provisioning",
                                      "actor": "harness", "authoring_mutation": True}
        self.rejected()

    def test_declared_fault_is_allowed_only_in_recovery_by_harness(self):
        self.attempt["actions"][0] = {"origin": "declared_fault", "actor": "harness",
                                      "authoring_mutation": True, "phase": "recovery",
                                      "fault_id": "external_edit"}
        self.validate()
        self.attempt["actions"][0]["actor"] = "model"
        self.rejected()

    def test_final_aggregation_rejects_missing_attempts(self):
        with self.assertRaises(ValueError):
            lab.aggregate([self.attempt], self.freeze, self.registry)

    def test_duplicate_best_of_attempt_rejected(self):
        with self.assertRaises(ValueError):
            lab.aggregate([self.attempt, self.attempt], self.freeze, self.registry)

    def test_all_attempts_preserve_failed_and_blocked_denominators(self):
        rows = []
        for scheduled in lab.schedule(self.freeze, self.registry):
            row = copy.deepcopy(self.attempt)
            row.update({k: scheduled[k] for k in ("task_id", "study", "seed", "arm")})
            task = next(t for t in self.registry["tasks"] if t["id"] == row["task_id"])
            for phase in row["phases"]:
                phase["checks"] = [{"name": name, "native": True, "outcome": "PASS",
                                    "evidence_sha256": "7"*64} for name in task["native_oracles"]]
            row["metrics"]["user_wall_ms"] = {"value": 1500 if row["arm"] == "semantic" else 1000,
                                                  "reason": None}
            if row["arm"] == "low_level":
                row["outcome"] = "BLOCKED"
                for phase in row["phases"]:
                    phase["outcome"] = "BLOCKED"
            rows.append(row)
        result = lab.aggregate(rows, self.freeze, self.registry)
        self.assertEqual(result["attempts"], 60)
        self.assertEqual(result["counts"]["S"]["low_level"]["BLOCKED"], 10)
        self.assertEqual({r["semantic_minus_direct_user_wall_ms"]
                          for r in result["paired_all_attempts"]}, {500})
        self.assertIsNone(result["winner_claim"])
        self.assertFalse(result["statistical_significance_claimed"])

    def test_duplicate_json_keys_rejected(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp)/"duplicate.json"
            path.write_text('{"outcome":"FAILED","outcome":"COMPLETED"}')
            with self.assertRaises(ValueError):
                lab.load(path)


if __name__ == "__main__":
    unittest.main()
