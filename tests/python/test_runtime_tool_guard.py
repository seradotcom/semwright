import shutil
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
    def test_integration_driver_sources_are_inside_runtime_resolver_ratchet(self):
        fixture_root = ROOT / "integrations" / "__runtime_guard_test__"
        source = fixture_root / "driver" / "src" / "main.rs"
        self.assertFalse(fixture_root.exists())
        source.parent.mkdir(parents=True)
        source.write_text('const FORBIDDEN: &str = "/usr/bin/ambient-runtime";\n')
        try:
            result = run_guard()
            self.assertNotEqual(result.returncode, 0)
            self.assertIn(
                "integrations/__runtime_guard_test__/driver/src/main.rs",
                result.stderr,
            )
            self.assertIn("/usr/bin/ambient-runtime", result.stderr)
        finally:
            shutil.rmtree(fixture_root)

        clean = run_guard()
        self.assertEqual(clean.returncode, 0, clean.stderr)
        self.assertIn("tracked legacy entries=0", clean.stdout)


if __name__ == "__main__":
    unittest.main()
