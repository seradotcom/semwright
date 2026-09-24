from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parents[2]
PACKET = (ROOT / "docs" / "security-review.md").read_text()


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


if __name__ == "__main__":
    unittest.main()
