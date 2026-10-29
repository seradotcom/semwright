"""Adversarial checks for stale handoffs and uncertain outcomes; no native app calls."""
from pathlib import Path
import copy
import json
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import Mock, patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
import godot_handoff_guard as guard


class HandoffGuardTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="archipelago-guard-")
        self.root = Path(self.temp.name)
        self.addCleanup(self.temp.cleanup)

    def baseline(self):
        stamp = {"revision": 6, "fingerprint": "fixture"}
        camera = {"ref": "native:old", "properties": {"fov": 60},
                  "observed": {"position": [0, 0, 0]}}
        cfg = {"editor_session": "session", "project_id": "project",
               "supporting_scene": "res://proofs/support.tscn",
               "initial_scene": "res://main.tscn", "reconciled_stamp": stamp,
               "reconciled_camera_data": camera}
        sessions = {"sessions": [{"session": "session", "project": "project",
                     "generation": "generation", "engine_version": "4.7.2",
                     "plugin_version": "0.2.0"}]}
        scene = {"stamp": stamp, "data": {"scene": cfg["supporting_scene"],
                                         "nodes": [{"name": "ProofCamera"}]}}
        baseline = {"sessions": sessions, "scene": scene}
        observed = {"sessions": copy.deepcopy(sessions), "scene": copy.deepcopy(scene),
                    "camera": {"stamp": stamp, "data": copy.deepcopy(camera)},
                    "project": {"data": {"scene": cfg["supporting_scene"]}},
                    "editor": {"data": {"scene": cfg["supporting_scene"], "playing": False,
                        "unsaved_scenes": [cfg["initial_scene"], cfg["supporting_scene"]]}},
                    "assets": {"data": {"scanning": False, "importing": False}}}
        observed["camera"]["data"]["ref"] = "native:new"
        return cfg, baseline, observed

    def test_live_projection_ignores_only_ephemeral_ref(self):
        cfg, baseline, observed = self.baseline()
        guard.validate_live(cfg, baseline, observed)

    def test_rejects_live_generation_revision_camera_and_unsaved_drift(self):
        changes = (
            lambda x: x["sessions"]["sessions"][0].update(generation="restarted"),
            lambda x: x["scene"].update(stamp={"revision": 7, "fingerprint": "new"}),
            lambda x: x["camera"]["data"]["properties"].update(fov=75),
            lambda x: x["editor"]["data"].update(unsaved_scenes=["res://main.tscn"]),
            lambda x: x["project"]["data"].update(scene="res://main.tscn"),
            lambda x: x["assets"]["data"].update(importing=True),
        )
        for change in changes:
            with self.subTest(change=changes.index(change)):
                cfg, baseline, observed = self.baseline()
                change(observed)
                with self.assertRaises(guard.PreflightError):
                    guard.validate_live(cfg, baseline, observed)

    def test_reused_pid_is_not_process_continuity(self):
        original = {"pid": 123, "created": "old", "path": r"E:\bin\godot.exe", "session": 1}
        reused = dict(original, created="new")
        with self.assertRaisesRegex(guard.PreflightError, "birth"):
            guard.check_process_identity(original, reused)

    def test_preflight_never_accepts_mutation_command(self):
        with patch.object(guard.subprocess, "run") as run:
            with self.assertRaisesRegex(guard.PreflightError, "read operations only"):
                guard.observe("cli", "pipe", "session", self.root,
                              "illegal", "scene.save", {})
            run.assert_not_called()
        self.assertEqual(list(self.root.iterdir()), [])

    def test_inventory_fails_before_descending_reparse_directory(self):
        (self.root / "junction").mkdir()
        with patch.object(guard, "is_reparse", side_effect=lambda p: p.name == "junction"):
            with self.assertRaisesRegex(guard.PreflightError, "Reparse source directory"):
                guard.inventory(self.root)

    def test_inventory_does_not_silence_directory_access_errors(self):
        def broken_walk(root, *, followlinks, onerror):
            onerror(PermissionError("unreadable new subtree"))
            return iter(())
        with patch.object(guard.os, "walk", side_effect=broken_walk):
            with self.assertRaises(PermissionError):
                guard.inventory(self.root)

    def test_inventory_excludes_only_root_editor_cache(self):
        (self.root / ".godot").mkdir()
        (self.root / ".godot" / "cache").write_text("cache")
        (self.root / "nested" / ".godot").mkdir(parents=True)
        (self.root / "nested" / ".godot" / "source").write_text("source")
        self.assertEqual(set(guard.inventory(self.root)), {"nested/.godot/source"})

    def put_pair(self, label, command, *, ok=True, applied=True,
                 error=None, request_id=None, response=True, backend="driver:godot"):
        command = "driver.godot." + command
        guard.save_new(self.root / (label + ".request.json"),
                       {"request_id": label, "command": command})
        if response:
            guard.save_new(self.root / (label + ".envelope.json"), {
                "request_id": request_id or label, "command": command, "ok": ok,
                "data": {"applied": applied}, "error": error,
                "execution": {"backend": backend, "policy_decision":
                    ("allow_after_confirmation" if command in {
                     "driver.godot.asset.reimport", "driver.godot.scene.open"} else "allow")},
            })

    def test_missing_mismatched_and_uncertain_envelopes_remain_unknown(self):
        self.put_pair("01", "scene.save", response=False)
        self.put_pair("02", "scene.save", request_id="wrong")
        self.put_pair("03", "scene.save", ok=False,
                      error={"code": "Unsupported", "outcome_known": False})
        self.assertEqual([row["classification"] for row in guard.classify_requests(self.root)],
                         ["UNKNOWN"] * 3)

    def test_unapplied_or_wrong_backend_success_is_not_proof(self):
        self.put_pair("01", "scene.save", applied=False)
        self.put_pair("02", "scene.save", backend="other")
        rows = guard.classify_requests(self.root)
        self.assertEqual([row["classification"] for row in rows], ["UNKNOWN", "UNKNOWN"])

    def test_later_denial_does_not_erase_applied_mutation(self):
        self.put_pair("01", "asset.reimport")
        self.put_pair("02", "scene.open", ok=False, applied=False,
                      error={"code": "ConsentRequired", "outcome_known": True})
        rows = guard.classify_requests(self.root)
        state = guard.aggregate_outcomes(rows, {"client_started": True}, False)
        self.assertEqual(state, "PARTIAL_KNOWN")
        self.assertEqual(rows[1]["classification"], "DENIED")

    def test_successful_reads_do_not_turn_denial_into_partial_mutation(self):
        self.put_pair("01", "scene.inspect")
        self.put_pair("02", "scene.open", ok=False, applied=False,
                      error={"code": "ConsentRequired", "outcome_known": True})
        self.assertEqual(guard.aggregate_outcomes(
            guard.classify_requests(self.root), {"client_started": True}, False), "DENIED")

    def test_client_timeout_does_not_claim_broker_cancelled(self):
        child = Mock(pid=999)
        child.wait.side_effect = [subprocess.TimeoutExpired(["proof"], 1), -9]
        with patch.object(guard.subprocess, "Popen", return_value=child) as popen:
            result = guard.execute_client_once(["proof"], self.root, {},
                                               self.root / "proof.log", timeout=1)
        popen.assert_called_once()
        child.kill.assert_called_once()
        self.assertTrue(result["client_started"])
        self.assertTrue(result["client_timeout"])
        self.assertFalse(result["broker_operation_cancelled"])
        self.assertEqual(guard.aggregate_outcomes([], result, False), "UNKNOWN")

    def test_interrupted_client_is_reaped_without_claiming_broker_cancellation(self):
        for number, error in enumerate((KeyboardInterrupt(), OSError("wait failed"))):
            with self.subTest(error=type(error).__name__):
                child = Mock(pid=999)
                child.poll.return_value = None
                child.wait.side_effect = [error, -9]
                with patch.object(guard.subprocess, "Popen", return_value=child):
                    result = guard.execute_client_once(["proof"], self.root, {},
                        self.root / ("interrupted" + str(number) + ".log"))
                child.kill.assert_called_once()
                self.assertEqual(child.wait.call_count, 2)
                self.assertTrue(result["client_started"])
                self.assertFalse(result["broker_operation_cancelled"])
                self.assertEqual(guard.aggregate_outcomes([], result, False), "UNKNOWN")

    def test_launch_failure_is_not_submitted_and_log_survives(self):
        with patch.object(guard.subprocess, "Popen", side_effect=OSError("not executable")):
            result = guard.execute_client_once(["proof"], self.root, {},
                                               self.root / "proof.log")
        self.assertFalse(result["client_started"])
        self.assertEqual(guard.aggregate_outcomes([], result, False), "NOT_SUBMITTED")
        self.assertTrue((self.root / "proof.log").exists())

    def test_prior_submission_blocks_reads_and_writes_a_failure_receipt(self):
        console = self.root / "console"
        console.mkdir()
        guard.save_new(self.root / "attempt.prepared.json", {"state": "PREPARED_NOT_SUBMITTED"})
        guard.save_new(self.root / "attempt.json", {})
        guard.save_new(self.root / "prepared.json", {})
        guard.save_new(console / "receipt.json", {"ready_for_human": True,
                                                 "approval_prompts_submitted": 0})
        guard.save_new(console / "launch.json", {"native_approval_console": True})
        guard.save_new(self.root / "attempt.submission.json", {"previous": True})
        with patch.object(guard.subprocess, "run") as run:
            result = guard.run_preflight(self.root, self.root, self.root, console, "attempt")
            run.assert_not_called()
        self.assertFalse(result["passed"])
        self.assertIn("Earlier submission", result["error"])
        self.assertEqual(result["native_approval_requests_submitted"], 0)
        self.assertTrue((Path(result["evidence_directory"]) / "receipt.json").is_file())

    def test_optimized_interpreter_cannot_disable_runner_checks(self):
        runner = Path(__file__).resolve().parents[1] / "scripts" / "Run-GodotOperatorHandoff.py"
        result = subprocess.run([sys.executable, "-O", "-B", str(runner), "--preflight"],
                                capture_output=True, text=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Optimized Python", result.stderr)


if __name__ == "__main__":
    unittest.main(verbosity=2)
