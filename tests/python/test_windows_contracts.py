import json
import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
COMMANDS = json.loads((ROOT / "schemas/commands.json").read_text())
BY_NAME = {command["name"]: command for command in COMMANDS}
WINDOWS_SOURCE = (ROOT / "crates/platform-windows/src/lib.rs").read_text()


def windows_commands():
    match = re.search(
        r'pub const COMMANDS: &\[&str\] = &\[(.*?)\];',
        WINDOWS_SOURCE,
        re.S,
    )
    if not match:
        raise AssertionError("Windows COMMANDS catalog not found")
    return set(re.findall(r'"([^"]+)"', match.group(1)))


class WindowsContractTests(unittest.TestCase):
    def test_every_advertised_windows_command_names_windows_backend(self):
        for name in sorted(windows_commands()):
            self.assertIn(name, BY_NAME)
            self.assertIn("windows", BY_NAME[name].get("backends", []), name)

    def test_windows_input_surface_matches_portable_shapes(self):
        self.assertNotIn("input.key", windows_commands())
        self.assertEqual(
            set(BY_NAME["pointer.move"]["input_schema"]["required"]),
            {"ref", "dx", "dy"},
        )
        self.assertEqual(
            set(BY_NAME["pointer.click"]["input_schema"]["required"]),
            {"ref", "button"},
        )
        self.assertEqual(
            set(BY_NAME["pointer.scroll"]["input_schema"]["required"]),
            {"ref", "dx", "dy"},
        )

    def test_window_move_and_resize_remain_distinct_contracts(self):
        self.assertEqual(
            set(BY_NAME["window.move"]["input_schema"]["required"]),
            {"ref", "x", "y"},
        )
        self.assertEqual(
            set(BY_NAME["window.resize"]["input_schema"]["required"]),
            {"ref", "width", "height"},
        )

    def test_async_close_outcome_is_explicitly_allowed(self):
        variants = BY_NAME["window.close"]["output_schema"]["oneOf"]
        self.assertTrue(any(
            set(v.get("required", [])) == {"requested", "delivery_verified"}
            for v in variants
        ))


if __name__ == "__main__":
    unittest.main()
