import subprocess
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
GUARD = ROOT / "scripts" / "verify-driver-runtime-tools.py"


def run_guard() -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, str(GUARD)],
        cwd=ROOT,
        text=True,
        capture_output=True,
        check=False,
    )


class RuntimeToolGuardTests(unittest.TestCase):
    def assert_guard_rejects(self, source: Path, relative: str) -> None:
        self.assertFalse(source.exists())
        source.parent.mkdir(parents=True, exist_ok=True)
        source.write_text('const forbidden = "/usr/bin/ambient-runtime";\n')
        try:
            result = run_guard()
            self.assertNotEqual(result.returncode, 0)
            self.assertIn(relative, result.stderr)
            self.assertIn("/usr/bin/ambient-runtime", result.stderr)
        finally:
            source.unlink()
            parent = source.parent
            while parent != ROOT and not any(parent.iterdir()):
                parent.rmdir()
                parent = parent.parent

        clean = run_guard()
        self.assertEqual(clean.returncode, 0, clean.stderr)
        self.assertIn("tracked legacy entries=0", clean.stdout)

    def test_integration_driver_sidecar_is_inside_runtime_resolver_ratchet(self):
        source = (
            ROOT
            / "integrations"
            / "__runtime_guard_test__"
            / "driver"
            / "src"
            / "runtime.py"
        )
        self.assert_guard_rejects(
            source,
            "integrations/__runtime_guard_test__/driver/src/runtime.py",
        )

    def test_driver_plugin_sidecar_is_inside_runtime_resolver_ratchet(self):
        source = (
            ROOT
            / "crates"
            / "driver-figma"
            / "plugin"
            / "src"
            / "__runtime_guard_test__.ts"
        )
        self.assert_guard_rejects(
            source,
            "crates/driver-figma/plugin/src/__runtime_guard_test__.ts",
        )


if __name__ == "__main__":
    unittest.main()
