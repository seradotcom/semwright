import importlib.util
import json
import hashlib
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("repository_secrets", ROOT / "scripts/ci/repository-secret-scan.py")
SCANNER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SCANNER)


class SecretScanEvidenceTests(unittest.TestCase):
    def test_report_drops_secret_bodies_and_identity_metadata(self):
        rows = SCANNER.sanitize_findings([{
            "RuleID": "synthetic", "File": "/tmp/snapshot/test.txt", "StartLine": 3,
            "EndLine": 4, "Commit": "a" * 40, "Match": "DO-NOT-PUBLISH",
            "Secret": "DO-NOT-PUBLISH", "Description": "DO-NOT-PUBLISH",
            "Author": "DO-NOT-PUBLISH", "Email": "DO-NOT-PUBLISH", "Message": "DO-NOT-PUBLISH",
        }], Path("/tmp/snapshot"))
        self.assertEqual(rows[0]["file"], "test.txt")
        self.assertNotIn("DO-NOT-PUBLISH", json.dumps(rows))
        self.assertEqual(set(rows[0]), {"rule_id", "file", "start_line", "end_line", "commit"})

    def test_nonsecret_triage_requires_rule_path_and_exact_line(self):
        line = '"commit": "public-upstream-identity"'
        entry = {"file": "fixture.json", "rule_id": "generic-api-key",
                 "line_sha256": hashlib.sha256(line.encode()).hexdigest(), "reason": "fixture"}
        finding = {"file": "fixture.json", "rule_id": "generic-api-key", "start_line": 2, "end_line": 2}
        self.assertEqual(SCANNER.triage_metadata(finding, line, [entry])["classification"], "REVIEWED_NON_SECRET")
        self.assertIsNone(SCANNER.triage_metadata(finding, line + " changed", [entry]))
        self.assertIsNone(SCANNER.triage_metadata({**finding, "file": "other.json"}, line, [entry]))
        self.assertIsNone(SCANNER.triage_metadata({**finding, "rule_id": "other-rule"}, line, [entry]))
        self.assertIsNone(SCANNER.triage_metadata({**finding, "end_line": 3}, line, [entry]))

    def test_historical_triage_can_bind_hashed_path_and_commit(self):
        path = "historical/internal-record.json"
        line = '"commit": "public-merge-identity"'
        commit = "a" * 40
        entry = {
            "file_sha256": hashlib.sha256(path.encode()).hexdigest(),
            "commit": commit,
            "rule_id": "generic-api-key",
            "line_sha256": hashlib.sha256(line.encode()).hexdigest(),
            "reason": "historical public identity",
        }
        finding = {"file": path, "commit": commit, "rule_id": "generic-api-key",
                   "start_line": 2, "end_line": 2}
        self.assertEqual(
            SCANNER.triage_metadata(finding, line, [entry])["classification"],
            "REVIEWED_NON_SECRET",
        )
        self.assertIsNone(SCANNER.triage_metadata({**finding, "commit": "b" * 40}, line, [entry]))
        self.assertIsNone(SCANNER.triage_metadata({**finding, "file": "other.json"}, line, [entry]))

    def test_exit_code_and_findings_must_agree(self):
        self.assertEqual(SCANNER.scan_status(0, []), "PASS")
        self.assertEqual(SCANNER.scan_status(1, [{}]), "FINDINGS")
        for code, rows in [(1, []), (0, [{}]), (2, []), (124, []), (-9, [])]:
            self.assertEqual(SCANNER.scan_status(code, rows), "ERROR")

    def test_malformed_findings_are_rejected(self):
        with self.assertRaises(ValueError):
            SCANNER.sanitize_findings(["untrusted text"], Path("/tmp/snapshot"))

    def test_workflow_is_hosted_read_only_and_history_complete(self):
        text = (ROOT / ".github/workflows/repository-secret-scan.yml").read_text()
        self.assertIn("runs-on: ubuntu-24.04", text)
        self.assertIn("fetch-depth: 0", text)
        self.assertIn("persist-credentials: false", text)
        self.assertIn("contents: read", text)
        self.assertNotIn("self-hosted", text)
        self.assertNotIn("pull_request_target", text)
        self.assertNotIn("continue-on-error", text)
        self.assertIn("sha256sum -c -", text)


if __name__ == "__main__":
    unittest.main()
