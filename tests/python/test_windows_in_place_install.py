"""Windows helper contracts; inert fixture payloads are never executed."""
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SHELL = os.environ.get("SEMWRIGHT_TEST_POWERSHELL") or (
    shutil.which("pwsh") or shutil.which("powershell")
)
HELPERS = ("Install-Semwright.ps1", "Uninstall-Semwright.ps1", "Manage-Semwright.ps1")
BINS = ("semwright", "semwrightd", "semwright-mcp", "semwright-inspect", "semwright-sandbox")


@unittest.skipUnless(os.name == "nt" and SHELL, "requires native Windows PowerShell")
class WindowsInPlaceInstallTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(
            prefix="semwright-in-place-", dir=os.environ.get("SEMWRIGHT_INSTALL_TEST_ROOT")
        )
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.bundle = self.root / "Portable bundle with spaces"
        (self.bundle / "bin").mkdir(parents=True)
        self.parent_file = self.root / "unrelated-project.txt"
        self.parent_file.write_bytes(b"preserve the sibling project\n")
        for helper in HELPERS:
            shutil.copyfile(ROOT / "packaging/portable" / helper, self.bundle / helper)
        for name in BINS:
            (self.bundle / "bin" / (name + ".exe")).write_bytes(
                ("inert helper-contract payload: " + name + "\n").encode()
            )
        self.checksums()
        self.receipt = self.bundle / ".semwright-install-receipt"

    def checksums(self):
        rows = []
        for path in sorted(self.bundle.rglob("*")):
            if path.is_file() and path.name != "SHA256SUMS":
                rows.append(hashlib.sha256(path.read_bytes()).hexdigest()
                            + "  " + path.relative_to(self.bundle).as_posix())
        (self.bundle / "SHA256SUMS").write_text("\n".join(rows) + "\n", encoding="utf-8")

    def snapshot(self):
        return {p.relative_to(self.bundle).as_posix(): p.read_bytes()
                for p in self.bundle.rglob("*") if p.is_file()}

    def helper(self, operation, *arguments, success=True, bundle=None):
        script = (bundle or self.bundle) / (operation + "-Semwright.ps1")
        result = subprocess.run(
            [SHELL, "-NoProfile", "-NonInteractive", "-File", str(script), *map(str, arguments)],
            capture_output=True, text=True, errors="replace", timeout=20,
        )
        self.assertEqual(result.returncode == 0, success, result.stdout + result.stderr)
        return result.stdout + result.stderr

    def test_registration_and_own_helper_removal_preserve_unowned_files(self):
        before = self.snapshot()
        self.helper("Install", "-InPlace")
        self.assertEqual({k: v for k, v in self.snapshot().items()
                          if k != self.receipt.name}, before)
        rows = dict(line.split("  ", 1)[::-1] for line in
                    self.receipt.read_text(encoding="utf-8").splitlines())
        self.assertEqual(set(rows), set(before))
        for name, body in before.items():
            self.assertEqual(rows[name], hashlib.sha256(body).hexdigest())
        unknown = self.bundle / "user-project.txt"
        unknown.write_bytes(b"keep the user's project\n")
        self.helper("Uninstall")
        self.assertEqual(self.snapshot(), {"user-project.txt": b"keep the user's project\n"})
        self.assertEqual(self.parent_file.read_bytes(), b"preserve the sibling project\n")

    def test_clean_in_place_uninstall_removes_only_the_bundle(self):
        self.helper("Install", "-InPlace")
        self.helper("Uninstall")
        self.assertFalse(self.bundle.exists())
        self.assertTrue(self.parent_file.is_file())

    def test_changed_payload_refuses_removal_before_deleting_any_file(self):
        self.helper("Install", "-InPlace")
        (self.bundle / "bin/semwright.exe").write_bytes(b"locally changed file\n")
        before = self.snapshot()
        output = self.helper("Uninstall", success=False)
        self.assertIn("File changed; refusing operation", output)
        self.assertEqual(self.snapshot(), before)

    def test_registration_never_replaces_an_existing_receipt(self):
        self.helper("Install", "-InPlace")
        before = self.snapshot()
        self.helper("Install", "-InPlace", success=False)
        self.assertEqual(self.snapshot(), before)

    def test_registration_rejects_a_different_prefix(self):
        outside = self.root / "another-directory"
        before = self.snapshot()
        self.helper("Install", "-InPlace", "-Prefix", outside, success=False)
        self.assertFalse(outside.exists())
        self.assertEqual(self.snapshot(), before)

    def test_copy_install_still_refuses_a_prefix_outside_the_user_profile(self):
        outside = self.root / "copied-install"
        if outside.is_relative_to(Path.home()):
            self.skipTest("fixture root must be outside the actual user profile")
        output = self.helper("Install", "-Prefix", outside, success=False)
        self.assertIn("Install prefix must be a child of your user profile", output)
        self.assertFalse(outside.exists())

    def test_changed_source_is_not_registered(self):
        (self.bundle / "bin/semwright-mcp.exe").write_bytes(b"checksum mismatch\n")
        before = self.snapshot()
        output = self.helper("Install", "-InPlace", success=False)
        self.assertIn("File changed; refusing operation", output)
        self.assertEqual(self.snapshot(), before)
        self.assertFalse(self.receipt.exists())

    def test_incomplete_package_is_not_registered(self):
        (self.bundle / "bin/semwright-mcp.exe").unlink()
        self.checksums()
        output = self.helper("Install", "-InPlace", success=False)
        self.assertIn("Missing checksummed executable: semwright-mcp", output)
        self.assertFalse(self.receipt.exists())

    def test_receipt_cannot_escape_the_registered_directory(self):
        self.helper("Install", "-InPlace")
        with self.receipt.open("a", encoding="utf-8") as stream:
            stream.write(hashlib.sha256(self.parent_file.read_bytes()).hexdigest()
                         + "  ../unrelated-project.txt\n")
        before = self.snapshot()
        output = self.helper("Uninstall", success=False)
        self.assertIn("Unsafe receipt path", output)
        self.assertEqual(self.snapshot(), before)
        self.assertEqual(self.parent_file.read_bytes(), b"preserve the sibling project\n")

    def test_junction_alias_cannot_register_the_bundle(self):
        alias = self.root / "redirected-bundle"
        quote = lambda path: "'" + str(path).replace("'", "''") + "'"
        subprocess.run(
            [SHELL, "-NoProfile", "-NonInteractive", "-Command",
             "New-Item -ItemType Junction -Path " + quote(alias)
             + " -Target " + quote(self.bundle) + " -ErrorAction Stop | Out-Null"],
            capture_output=True, check=True, timeout=15,
        )
        self.addCleanup(lambda: os.rmdir(alias) if alias.exists() else None)
        output = self.helper("Install", "-InPlace", success=False, bundle=alias)
        self.assertIn("Reparse point refused", output)
        self.assertFalse(self.receipt.exists())


if __name__ == "__main__":
    unittest.main()
