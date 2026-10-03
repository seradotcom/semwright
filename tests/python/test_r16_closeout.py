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

SMOKE_SPEC = importlib.util.spec_from_file_location(
    "r16_smoke", ROOT / "scripts/review/r16-smoke.py"
)
SMOKE = importlib.util.module_from_spec(SMOKE_SPEC)
SMOKE_SPEC.loader.exec_module(SMOKE)


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

    def test_closure_without_separate_receipt_is_rejected(self):
        data = record()
        data["r16_closed"] = True
        with tempfile.TemporaryDirectory() as folder:
            self.assertTrue(MODULE.evidence_errors(data, Path(folder)))

    def test_closure_with_bound_separate_receipt_is_valid(self):
        data = record()
        data["r16_closed"] = True
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            evidence = root / "verification/r16-closeout/evidence"
            evidence.mkdir(parents=True)
            (evidence / "INDEPENDENT_R16_REVALIDATION_2026-10-03.json").write_text(json.dumps({
                "external_audit": False,
                "fix_sha": "4ef9a06e486cd8d2e3851c298e244435ecef3232",
                "disposition": {"R16": "CLOSED"},
            }))
            self.assertEqual(MODULE.evidence_errors(data, root), [])

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

    def test_fake_smoke_requires_structured_effect_and_audit(self):
        lines = [
            {"command": "doctor", "ok": True, "data": {"fake": True}},
            {"command": "ui.find", "ok": True, "data": {"count": 2}},
            {"command": "recipe.run", "ok": True, "data": {
                "completed": True,
                "outputs": {"changed": True},
                "steps": [{"ok": True}, {"ok": True}],
            }},
            {"command": "audit.tail", "ok": True, "data": {"events": [
                {"command": "ui.invoke", "phase": "finish", "ok": True}
            ]}},
        ]
        stdout = "\n".join(json.dumps(row) for row in lines)
        assertions = SMOKE.assert_fake_smoke(stdout)
        self.assertIn("recipe_completed_changed_true", assertions)
        self.assertIn("audit_ui_invoke_finish_ok", assertions)

    def test_fake_smoke_rejects_exit_success_without_effect_evidence(self):
        lines = [
            {"command": "doctor", "ok": True, "data": {"fake": True}},
            {"command": "ui.find", "ok": True, "data": {"count": 2}},
            {"command": "recipe.run", "ok": True, "data": {
                "completed": True,
                "outputs": {"changed": False},
                "steps": [{"ok": True}, {"ok": True}],
            }},
            {"command": "audit.tail", "ok": True, "data": {"events": []}},
        ]
        with self.assertRaises(ValueError):
            SMOKE.assert_fake_smoke("\n".join(json.dumps(row) for row in lines))

    def test_local_links_and_external_exclusion(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root / "exists.md").write_text("# Existing")
            (root / "README.md").write_text("[ok](exists.md) [bad](missing.md) [web](https://example.com)")
            errors = MODULE.links(root, "README.md")
            self.assertEqual(len(errors), 1)
            self.assertIn("missing.md", errors[0])

    def test_v1_closeout_keeps_environment_gaps_open_and_release_fail_closed(self):
        closeout = json.loads((ROOT / "verification/v1-engineering-closeout.json").read_text())
        self.assertEqual(closeout["V1_ENGINEERING_CLOSEOUT"], "COMPLETE")
        for gate in ("R06", "R18"):
            self.assertEqual(closeout[gate]["status"], "OPEN")
            self.assertEqual(
                closeout[gate]["disposition"],
                "DEFERRED_TO_POST_V1_ENVIRONMENT_DEPENDENT",
            )
            self.assertFalse(closeout[gate]["known_software_defect_behind_gap"])
        readiness = json.loads((ROOT / "release-readiness.json").read_text())
        self.assertEqual(readiness["status"], "BLOCKED_DEVELOPMENT_SOURCE")
        self.assertFalse(readiness["gates"]["live_desktop_matrix"])
        self.assertFalse(readiness["gates"]["security_review"])


if __name__ == "__main__":
    unittest.main()
