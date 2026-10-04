import json
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]

class RepositoryContractTests(unittest.TestCase):
    def test_rust_package_uses_workspace_license(self):
        text = (ROOT / "crates/native-sdk/Cargo.toml").read_text()
        self.assertIn("license.workspace = true", text)

    def test_typescript_package_declares_repository_license(self):
        package = json.loads((ROOT / "sdk/native-typescript/package.json").read_text())
        self.assertEqual(package.get("license"), "MIT OR Apache-2.0")

    def test_public_docs_are_present(self):
        docs = ROOT / "docs/native-sdk"
        for name in ["README.md", "QUICKSTART.md", "API.md", "VERIFY.md", "COMPATIBILITY.md"]:
            self.assertTrue((docs / name).is_file(), name)


if __name__ == "__main__":
    unittest.main()
