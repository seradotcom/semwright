from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parents[2]
RUNTIME = (ROOT / "crates" / "federation" / "src" / "lib.rs").read_text()
SMOKE = (ROOT / "scripts" / "dev" / "federation-smoke.sh").read_text()


class FederationSandboxContractTests(unittest.TestCase):
    def test_provenance_prefix_matches_runtime_and_smoke(self):
        marker = "sandboxed-stdio-sha256:"
        self.assertIn(marker, RUNTIME)
        self.assertIn(marker, SMOKE)
        self.assertNotIn("trusted-stdio-sha256:", SMOKE)

    def test_hosted_sandbox_gate_is_fail_closed(self):
        workflow = (
            ROOT / ".github" / "workflows" / "native-integrations.yml"
        ).read_text()
        self.assertIn("SEMWRIGHT_REQUIRE_MCP_SANDBOX", workflow)


if __name__ == "__main__":
    unittest.main()
