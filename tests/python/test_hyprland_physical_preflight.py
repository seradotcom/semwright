from pathlib import Path
import json
import os
import stat
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts" / "dev" / "hyprland-physical-preflight.sh"
ACK = "I_AM_ON_A_DISPOSABLE_PHYSICAL_HYPRLAND_LOGIN"


class HyprlandPhysicalPreflightTests(unittest.TestCase):
    def _fixture(self, nested=False, desktop="Hyprland"):
        tmp = tempfile.TemporaryDirectory()
        root = Path(tmp.name)
        bindir = root / "bin"
        proc = root / "proc" / "4242"
        bindir.mkdir()
        proc.mkdir(parents=True)

        hyprctl = bindir / "hyprctl"
        hyprctl.write_text(
            "#!/usr/bin/env bash\n"
            "if [[ \"$1\" == \"monitors\" ]]; then\n"
            "  printf '%s\\n' '[{\"name\":\"eDP-1\",\"scale\":1.0}]'\n"
            "elif [[ \"$1\" == \"clients\" ]]; then\n"
            "  printf '%s\\n' '[]'\n"
            "else\n"
            "  exit 3\n"
            "fi\n"
        )
        hyprctl.chmod(hyprctl.stat().st_mode | stat.S_IXUSR)

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
            "  *) exit 4 ;;\n"
            "esac\n"
        )
        loginctl.chmod(loginctl.stat().st_mode | stat.S_IXUSR)

        environ = b"PATH=/usr/bin\0XDG_SESSION_TYPE=wayland\0"
        if nested:
            environ += b"WAYLAND_DISPLAY=wayland-parent\0"
        (proc / "environ").write_bytes(environ)

        env = os.environ.copy()
        env.update(
            {
                "PATH": str(bindir) + os.pathsep + env["PATH"],
                "SEMWRIGHT_HYPR_PHYSICAL_ACK": ACK,
                "SEMWRIGHT_HYPR_SESSION_ID": "7",
                "SEMWRIGHT_HYPR_COMPOSITOR_PID": "4242",
                "SEMWRIGHT_PROC_ROOT": str(root / "proc"),
                "XDG_SESSION_TYPE": "wayland",
                "XDG_CURRENT_DESKTOP": desktop,
                "HYPRLAND_INSTANCE_SIGNATURE": "test-instance",
            }
        )
        return tmp, env

    def test_accepts_physical_hyprland_shape_without_certifying(self):
        tmp, env = self._fixture()
        with tmp:
            run = subprocess.run(
                [str(SCRIPT)],
                env=env,
                text=True,
                capture_output=True,
                check=True,
            )
        result = json.loads(run.stdout)
        self.assertEqual(result["status"], "PASS_PREFLIGHT_ONLY")
        self.assertFalse(result["certification_complete"])
        self.assertEqual(result["physical_monitor_names"], ["eDP-1"])

    def test_rejects_nested_compositor_parent(self):
        tmp, env = self._fixture(nested=True)
        with tmp:
            run = subprocess.run(
                [str(SCRIPT)],
                env=env,
                text=True,
                capture_output=True,
            )
        self.assertNotEqual(run.returncode, 0)
        self.assertIn("looks nested", run.stderr)

    def test_rejects_non_hyprland_desktop(self):
        tmp, env = self._fixture(desktop="ubuntu:GNOME")
        with tmp:
            run = subprocess.run(
                [str(SCRIPT)],
                env=env,
                text=True,
                capture_output=True,
            )
        self.assertNotEqual(run.returncode, 0)
        self.assertIn("desktop is not Hyprland", run.stderr)


if __name__ == "__main__":
    unittest.main()
