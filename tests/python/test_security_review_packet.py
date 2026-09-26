from pathlib import Path
import hashlib
import json
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
PACKET = (ROOT / "docs" / "security-review.md").read_text()
BUNDLE_SCRIPT = ROOT / "scripts" / "dev" / "security-review-bundle.sh"


class SecurityReviewPacketTests(unittest.TestCase):
    def test_plugin_hostile_suite_cannot_silently_select_zero_tests(self):
        self.assertIn("SEMWRIGHT_TEST_PLUGIN_SANDBOX=1", PACKET)
        self.assertIn(
            "cargo test --locked -p semwright-plugin-host --features test-tools",
            PACKET,
        )
        self.assertIn("--test adversarial -- --ignored --nocapture", PACKET)

    def test_driver_hostile_suite_uses_real_sandbox_opt_in(self):
        self.assertIn("SEMWRIGHT_TEST_DRIVER_SANDBOX=1", PACKET)
        self.assertIn("SEMWRIGHT_TEST_SANDBOX_HELPER=", PACKET)
        self.assertIn(
            "cargo test --locked -p semwright-driver-host --features test-tools",
            PACKET,
        )
        self.assertIn("--test adversarial_sandbox -- --ignored --nocapture", PACKET)

    def test_zero_selected_tests_are_explicitly_rejected_as_evidence(self):
        self.assertIn("running 0 tests", PACKET)
        self.assertIn("is not review evidence", PACKET)

    def test_reviewer_handoff_is_commit_scoped(self):
        self.assertIn("git archive --format=tar", PACKET)
        self.assertIn("gzip -n -9", PACKET)
        self.assertIn("UNREVIEWED", PACKET)
        self.assertIn("exact commit, not the maintainer working tree", PACKET)

    def test_green_ci_cannot_substitute_for_independent_conclusion(self):
        self.assertIn("green CI", PACKET)
        self.assertIn("cannot substitute for the independent reviewer conclusion", PACKET)

    def test_bundle_generator_is_commit_scoped_and_unreviewed(self):
        baseline = subprocess.check_output(
            ["git", "-C", str(ROOT), "rev-parse", "HEAD"], text=True
        ).strip()
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp) / "packet"
            subprocess.run(
                [str(BUNDLE_SCRIPT), baseline, str(out)],
                cwd=ROOT,
                check=True,
                text=True,
                capture_output=True,
            )
            manifest = json.loads((out / "manifest.json").read_text())
            self.assertEqual(manifest["baseline_sha"], baseline)
            self.assertEqual(manifest["status"], "UNREVIEWED")
            self.assertTrue(manifest["independent_review_required"])
            self.assertFalse(manifest["self_attestation"])
            archive = out / manifest["source_snapshot"]["archive"]
            digest = hashlib.sha256(archive.read_bytes()).hexdigest()
            self.assertEqual(digest, manifest["source_snapshot"]["sha256"])


if __name__ == "__main__":
    unittest.main()
