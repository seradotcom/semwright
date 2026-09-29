"""Catch builtin registry construction drift before a Rust compilation is needed."""
import json
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]


class BuiltinProvenanceSyncTests(unittest.TestCase):
    def test_every_descriptor_has_exact_explicit_provenance(self):
        commands = json.loads((ROOT / "schemas/commands.json").read_text())
        provenance = json.loads((ROOT / "schemas/builtin-provenance.json").read_text())
        names = [command["name"] for command in commands]
        self.assertTrue(names, "An empty catalog is not a valid parity check")
        self.assertEqual(len(names), len(set(names)), "Duplicate builtin descriptor")
        self.assertEqual(
            set(names), set(provenance),
            "Builtin command/provenance drift: update the explicit owner-reviewed "
            "metadata together with the command. Never infer authority for an unknown name.",
        )


if __name__ == "__main__":
    unittest.main()
