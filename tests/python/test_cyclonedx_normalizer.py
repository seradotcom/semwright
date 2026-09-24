import json
import re
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts" / "release"))

from normalize_cyclonedx import deterministic_serial, normalize  # noqa: E402

SERIAL_RE = re.compile(
    r"^urn:uuid:[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-"
    r"[0-9a-f]{4}-[0-9a-f]{12}$"
)


def document(target: str = "x86_64-unknown-linux-gnu") -> dict:
    return {
        "bomFormat": "CycloneDX",
        "specVersion": "1.5",
        "version": 1,
        "metadata": {
            "component": {"type": "application", "name": "semwright-cli"},
            "properties": [
                {
                    "name": "cdx:rustc:sbom:target:triple",
                    "value": target,
                }
            ],
        },
        "components": [],
        "dependencies": [],
    }


class CycloneDxNormalizerTests(unittest.TestCase):
    def test_normalization_is_deterministic_and_attest_compatible(self) -> None:
        commit = "a" * 40
        with tempfile.TemporaryDirectory() as tmp:
            first = Path(tmp) / "first.json"
            second = Path(tmp) / "second.json"
            payload = document()
            first.write_text(json.dumps(payload), encoding="utf-8")
            second.write_text(json.dumps(payload), encoding="utf-8")
            one = normalize(first, "semwright", commit)
            two = normalize(second, "semwright", commit)
            self.assertEqual(one, two)
            self.assertRegex(one, SERIAL_RE)
            self.assertEqual(first.read_bytes(), second.read_bytes())

            parsed = json.loads(first.read_text(encoding="utf-8"))
            self.assertEqual(parsed["bomFormat"], "CycloneDX")
            self.assertEqual(parsed["specVersion"], "1.5")
            self.assertTrue(parsed["serialNumber"])

    def test_identity_changes_with_commit_target_or_binary(self) -> None:
        base = deterministic_serial(
            "semwright", "a" * 40, "x86_64-unknown-linux-gnu"
        )
        self.assertNotEqual(
            base,
            deterministic_serial(
                "semwright", "b" * 40, "x86_64-unknown-linux-gnu"
            ),
        )
        self.assertNotEqual(
            base,
            deterministic_serial(
                "semwright", "a" * 40, "aarch64-unknown-linux-gnu"
            ),
        )
        self.assertNotEqual(
            base,
            deterministic_serial(
                "semwrightd", "a" * 40, "x86_64-unknown-linux-gnu"
            ),
        )

    def test_missing_target_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "bom.json"
            payload = document()
            payload["metadata"]["properties"] = []
            path.write_text(json.dumps(payload), encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "target triple"):
                normalize(path, "semwright", "a" * 40)


if __name__ == "__main__":
    unittest.main()
