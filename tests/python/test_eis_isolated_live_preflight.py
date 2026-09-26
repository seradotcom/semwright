from pathlib import Path
import json
import os
import stat
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts" / "dev" / "eis-isolated-live-preflight.sh"
ACK = "I_AM_IN_A_DISPOSABLE_VM_OR_INDEPENDENT_SEAT"


class EisIsolatedLivePreflightTests(unittest.TestCase):
    def _fixture(self, virt="none", seat="seat0", desktop="GNOME"):
        tmp = tempfile.TemporaryDirectory()
        root = Path(tmp.name)
        bindir = root / "bin"
        bindir.mkdir()

        detect = bindir / "systemd-detect-virt"
        detect.write_text(
            "#!/usr/bin/env bash\n"
            "if [[ \"$FAKE_VIRT\" == \"none\" ]]; then echo none; exit 1; fi\n"
            "echo \"$FAKE_VIRT\"\n"
        )
        detect.chmod(detect.stat().st_mode | stat.S_IXUSR)

        loginctl = bindir / "loginctl"
        loginctl.write_text(
            "#!/usr/bin/env bash\n"
            "if [[ \"$1\" == \"show-user\" ]]; then echo 7; exit 0; fi\n"
            "if [[ \"$1\" != \"show-session\" ]]; then exit 3; fi\n"
            "case \"$4\" in\n"
            "  Active) echo yes ;;\n"
            "  Type) echo wayland ;;\n"
            "  Remote) echo no ;;\n"
            "  Class) echo user ;;\n"
            "  State) echo active ;;\n"
            "  Seat) echo \"$FAKE_SEAT\" ;;\n"
            "  *) exit 4 ;;\n"
            "esac\n"
        )
        loginctl.chmod(loginctl.stat().st_mode | stat.S_IXUSR)

        env = os.environ.copy()
        env.update(
            {
                "PATH": str(bindir) + os.pathsep + env["PATH"],
                "FAKE_VIRT": virt,
                "FAKE_SEAT": seat,
                "SEMWRIGHT_EIS_ISOLATION_ACK": ACK,
                "SEMWRIGHT_EIS_SESSION_ID": "7",
                "XDG_SESSION_TYPE": "wayland",
                "XDG_CURRENT_DESKTOP": desktop,
            }
        )
        return tmp, env

    def test_accepts_vm_even_on_seat0(self):
        tmp, env = self._fixture(virt="kvm", seat="seat0")
        with tmp:
            run = subprocess.run(
                [str(SCRIPT)], env=env, text=True, capture_output=True, check=True
            )
        result = json.loads(run.stdout)
        self.assertEqual(result["status"], "PASS_PREFLIGHT_ONLY")
        self.assertEqual(result["isolation_mode"], "vm")
        self.assertFalse(result["certification_complete"])

    def test_accepts_independent_bare_metal_seat(self):
        tmp, env = self._fixture(virt="none", seat="seat1")
        with tmp:
            run = subprocess.run(
                [str(SCRIPT)], env=env, text=True, capture_output=True, check=True
            )
        result = json.loads(run.stdout)
        self.assertEqual(result["isolation_mode"], "independent-seat")
        self.assertEqual(result["seat"], "seat1")

    def test_rejects_bare_metal_seat0(self):
        tmp, env = self._fixture(virt="none", seat="seat0")
        with tmp:
            run = subprocess.run([str(SCRIPT)], env=env, text=True, capture_output=True)
        self.assertNotEqual(run.returncode, 0)
        self.assertIn("owner-active boundary", run.stderr)

    def test_can_require_expected_desktop(self):
        tmp, env = self._fixture(virt="kvm", desktop="KDE")
        env["SEMWRIGHT_EIS_EXPECT_DESKTOP"] = "GNOME"
        with tmp:
            run = subprocess.run([str(SCRIPT)], env=env, text=True, capture_output=True)
        self.assertNotEqual(run.returncode, 0)
        self.assertIn("does not match expected", run.stderr)


if __name__ == "__main__":
    unittest.main()
