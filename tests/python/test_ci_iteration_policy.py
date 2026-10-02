from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]
WORKFLOWS = ROOT / ".github/workflows"


class CiIterationPolicyTests(unittest.TestCase):
    def test_global_pr_workflows_have_scope_jobs(self):
        for name in ["ci.yml", "native-integrations.yml", "security.yml"]:
            text = (WORKFLOWS / name).read_text()
            self.assertIn("  scope:\n", text, name)
            self.assertIn("ci-affected-areas.py", text, name)

    def test_packaging_and_supply_chain_are_final_only(self):
        for name in ["packaging-certification.yml", "supply-chain.yml"]:
            head = (WORKFLOWS / name).read_text().split("jobs:", 1)[0]
            self.assertIn("workflow_call:", head, name)
            self.assertIn("workflow_dispatch:", head, name)
            self.assertNotIn("pull_request:", head, name)
            self.assertNotIn("push:\n", head, name)

    def test_expensive_platform_live_workflows_are_path_scoped(self):
        for name in ["platformization-macos.yml", "live-x11-ewmh.yml", "live-plasma.yml"]:
            head = (WORKFLOWS / name).read_text().split("jobs:", 1)[0]
            self.assertIn("pull_request:\n    paths:\n", head, name)
            self.assertIn("workflow_call:", head, name)


if __name__ == "__main__":
    unittest.main()
