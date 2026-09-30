from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github/workflows/candidate-certification.yml"


class CandidateCertificationWorkflowTests(unittest.TestCase):
    def test_exact_sha_binding_and_global_gates_are_present(self):
        text = WORKFLOW.read_text()
        self.assertIn('test "$(git rev-parse HEAD)" = "$SOURCE_SHA"', text)
        self.assertIn('git merge-base --is-ancestor "$BASE_SHA" "$SOURCE_SHA"', text)
        for workflow in [
            "ci.yml",
            "native-integrations.yml",
            "security.yml",
            "packaging-certification.yml",
            "supply-chain.yml",
            "pre-r16.yml",
        ]:
            self.assertIn(f"uses: ./.github/workflows/{workflow}", text)

    def test_evidence_manifest_is_uploaded(self):
        text = WORKFLOW.read_text()
        self.assertIn("CERTIFICATION.json", text)
        self.assertIn("candidate-certification-${{ inputs.source_sha }}", text)


if __name__ == "__main__":
    unittest.main()
