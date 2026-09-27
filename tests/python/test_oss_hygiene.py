import importlib.util
import pathlib
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts" / "verify-oss-hygiene.py"
SPEC = importlib.util.spec_from_file_location("verify_oss_hygiene", SCRIPT)
assert SPEC and SPEC.loader
MOD = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MOD)


class OssHygieneTests(unittest.TestCase):
    def scan(self, text: str):
        return MOD.scan_text(ROOT / "fixture.txt", text)

    def test_allows_documented_placeholders_and_ci_accounts(self):
        text = "\n".join(
            [
                "/home/" + "YOUR_USER" + "/.config/example",
                "/home/" + "runner" + "/work/repo",
                "C:\\Users\\" + "owner" + "\\workspace",
            ]
        )
        self.assertEqual(self.scan(text), [])

    def test_rejects_person_specific_absolute_homes(self):
        unix = "/home/" + "developername" + "/workspace"
        windows = "C:\\Users\\" + "developername" + "\\workspace"
        self.assertTrue(self.scan(unix))
        self.assertTrue(self.scan(windows))

    def test_rejects_session_specific_verification_language(self):
        phrases = [
            "this " + "hand" + "off",
            "source " + "dr" + "op",
            "## " + "Local " + "verification",
            "not run " + "locally",
        ]
        for phrase in phrases:
            with self.subTest(phrase=phrase):
                self.assertTrue(self.scan(phrase))

    def test_allows_architectural_local_terms(self):
        text = "The local runtime exposes a loopback-only bridge and local IPC endpoint."
        self.assertEqual(self.scan(text), [])


if __name__ == "__main__":
    unittest.main()
