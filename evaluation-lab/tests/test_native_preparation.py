"""Synthetic preflight/provenance controls, not native or model measurements."""
import copy
import hashlib
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
sys.path.insert(0, str(ROOT/"native"))
from program import stages, PHASES
from records import inventory, verify_chain
from admission import admit_model_session
from test_evaluate import fixtures


class NativePreparationTests(unittest.TestCase):
    def setUp(self):
        self.registry = json.loads((ROOT/"tasks/public-dev.json").read_text())

    def test_revision_sequence_owns_one_task_and_does_not_change_registry(self):
        before = copy.deepcopy(self.registry)
        for task in self.registry["tasks"]:
            if task["family"] in ("blender", "godot"):
                program = stages(task)
                self.assertEqual([s["phase"] for s in program], list(PHASES))
                self.assertEqual({s["task_id"] for s in program}, {task["id"]})
        self.assertEqual(self.registry, before)

    def test_real_revisions_change_color_rule_and_source_separately(self):
        for task in self.registry["tasks"]:
            if task["family"] in ("blender", "godot"):
                program = stages(task)
                self.assertNotEqual(program[0]["color"], program[1]["color"])
                rule = "rotation" if task["family"] == "blender" else "objective_count"
                self.assertNotEqual(program[1][rule], program[2][rule])
                extent = "width" if task["family"] == "blender" else "asset_scale"
                self.assertNotEqual(program[2][extent], program[3][extent])

    def test_native_program_cannot_turn_missing_family_into_pass(self):
        for task in self.registry["tasks"]:
            if task["family"] not in ("blender", "godot"):
                with self.assertRaises(ValueError):
                    stages(task)

    def test_parameter_bounds_reject_unbounded_native_work(self):
        task = copy.deepcopy(self.registry["tasks"][0])
        for value in (True, 0, 500, "4"):
            task["parameters"]["objective_count"] = value
            with self.assertRaises(ValueError):
                stages(task)

    def test_recovery_preserves_last_accepted_spec(self):
        for task in self.registry["tasks"]:
            if task["family"] in ("blender", "godot"):
                a, b = stages(task)[-2:]
                a.pop("phase"); b.pop("phase")
                self.assertEqual(a, b)

    def test_read_only_inventory_detects_same_size_external_edit(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/"user-note"
            path.write_text("before")
            before = inventory(directory)
            path.write_text("after!")
            self.assertNotEqual(inventory(directory), before)

    def test_actor_symlink_cannot_hide_foreign_resource(self):
        with tempfile.TemporaryDirectory() as directory:
            (Path(directory)/"foreign").symlink_to("/etc/hosts")
            with self.assertRaises(ValueError):
                inventory(directory)

    def test_chain_refuses_edit_reorder_missing_event_and_wrong_target(self):
        identity = {"source_sha": "synthetic-target"}
        previous = "0"*64
        events = []
        for i in range(3):
            event = {"sequence": i, "previous_sha256": previous, "identity": identity, "returncode": 0}
            previous = hashlib.sha256(json.dumps(event, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
            event["event_sha256"] = previous
            events.append(event)
        self.assertEqual(verify_chain(events, identity), previous)
        changed = copy.deepcopy(events); changed[1]["returncode"] = 1
        for invalid in (changed, events[1:], list(reversed(events)), []):
            with self.assertRaises(ValueError):
                verify_chain(invalid, identity)
        with self.assertRaises(ValueError):
            verify_chain(events, {"source_sha":"different"})

    def test_same_uid_model_cannot_modify_controller_evidence(self):
        registry, freeze, *_ = fixtures()
        with self.assertRaisesRegex(ValueError, "isolated"):
            admit_model_session(freeze, registry, {}, os.getuid())

    def test_model_admission_rejects_absent_user_authorization(self):
        registry, freeze, *_ = fixtures()
        with self.assertRaisesRegex(ValueError, "authorized model"):
            admit_model_session(freeze, registry, {}, os.getuid()+1000)

    def test_model_admission_rejects_wrong_identity_and_unapproved_budget(self):
        registry, freeze, *_ = fixtures()
        adapter = {"authorization":"USER_CONFIGURED_MODEL_ACCESS", "model_identity":"wrong"}
        with self.assertRaisesRegex(ValueError, "identity"):
            admit_model_session(freeze, registry, adapter, os.getuid()+1000)
        adapter.update(model_identity=freeze["model_identity"], config_digest=freeze["model_config_digest"], budget_authorized=False)
        with self.assertRaisesRegex(ValueError, "budget"):
            admit_model_session(freeze, registry, adapter, os.getuid()+1000)

    def test_unverified_adapter_is_not_a_finished_model_session(self):
        registry, freeze, *_ = fixtures()
        adapter = {"authorization":"USER_CONFIGURED_MODEL_ACCESS", "model_identity":freeze["model_identity"],
                   "config_digest":freeze["model_config_digest"],"budget_authorized":True}
        with self.assertRaisesRegex(ValueError, "not certified"):
            admit_model_session(freeze, registry, adapter, os.getuid()+1000)


if __name__ == "__main__":
    unittest.main()
