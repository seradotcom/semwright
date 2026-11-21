"""Windows install helper contracts with isolated files and one test-owned process."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time
import unittest

ROOT = Path(__file__).resolve().parents[2]
SHELL = os.environ.get("SEMWRIGHT_TEST_POWERSHELL") or (
    shutil.which("pwsh") or shutil.which("powershell")
)
HELPERS = ("Install-Semwright.ps1", "Uninstall-Semwright.ps1", "Manage-Semwright.ps1")
BINS = ("semwright", "semwrightd", "semwright-mcp", "semwright-inspect", "semwright-sandbox")
# Python must not forward PowerShell 7 module paths into Windows PowerShell 5.1.
# Each child edition constructs its own default module search path at startup.
PS_ENV = {key: value for key, value in os.environ.items() if key.upper() != "PSMODULEPATH"}


@unittest.skipUnless(os.name == "nt" and SHELL, "requires native Windows PowerShell")
class WindowsInPlaceInstallTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.owner_sid = subprocess.check_output(
            [SHELL, "-NoProfile", "-NonInteractive", "-Command",
             "[Security.Principal.WindowsIdentity]::GetCurrent().User.Value"],
            env=PS_ENV, text=True, timeout=15,
        ).strip()
        if not cls.owner_sid.startswith("S-1-"):
            raise RuntimeError("Cannot identify the test fixture owner")

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(
            prefix="semwright-in-place-", dir=os.environ.get("SEMWRIGHT_INSTALL_TEST_ROOT")
        )
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        # Only the disposable fixture changes permissions; ancestors are never changed.
        subprocess.run(
            ["icacls", str(self.root), "/inheritance:r", "/grant:r",
             f"*{self.owner_sid}:(OI)(CI)F", "*S-1-5-18:(OI)(CI)F",
             "*S-1-5-32-544:(OI)(CI)F"],
            env=PS_ENV, capture_output=True, check=True, timeout=15,
        )
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
            env=PS_ENV, capture_output=True, text=True, errors="replace", timeout=20,
        )
        self.assertEqual(result.returncode == 0, success, result.stdout + result.stderr)
        return result.stdout + result.stderr

    def acl(self, path, *, sid=None, rights=None, restore=None, template=False):
        code = r"""
$ErrorActionPreference = 'Stop'
$acl = Get-Acl -LiteralPath $env:SEMWRIGHT_ACL_PATH
if ($env:SEMWRIGHT_ACL_RESTORE) {
    $acl.SetSecurityDescriptorSddlForm($env:SEMWRIGHT_ACL_RESTORE)
    Set-Acl -LiteralPath $env:SEMWRIGHT_ACL_PATH -AclObject $acl
} elseif ($env:SEMWRIGHT_ACL_SID) {
    $rule = [Security.AccessControl.FileSystemAccessRule]::new(
        [Security.Principal.SecurityIdentifier]::new($env:SEMWRIGHT_ACL_SID),
        [Security.AccessControl.FileSystemRights]$env:SEMWRIGHT_ACL_RIGHTS,
        [Security.AccessControl.InheritanceFlags]$env:SEMWRIGHT_ACL_INHERITANCE,
        [Security.AccessControl.PropagationFlags]$env:SEMWRIGHT_ACL_PROPAGATION,
        [Security.AccessControl.AccessControlType]::Allow)
    $acl.AddAccessRule($rule)
    Set-Acl -LiteralPath $env:SEMWRIGHT_ACL_PATH -AclObject $acl
} else { $acl.Sddl }
"""
        return subprocess.check_output(
            [SHELL, "-NoProfile", "-NonInteractive", "-Command", code],
            env={**PS_ENV, "SEMWRIGHT_ACL_PATH": str(path),
                 "SEMWRIGHT_ACL_SID": sid or "", "SEMWRIGHT_ACL_RIGHTS": rights or "",
                 "SEMWRIGHT_ACL_RESTORE": restore or "",
                 "SEMWRIGHT_ACL_INHERITANCE": "ContainerInherit, ObjectInherit" if template else "None",
                 "SEMWRIGHT_ACL_PROPAGATION": "InheritOnly" if template else "None"},
            text=True, timeout=15,
        ).strip()

    def test_read_only_preflight_preserves_new_and_registered_bundles(self):
        before = self.snapshot()
        acls = {p: self.acl(p) for p in (self.root, self.bundle, self.bundle / "SHA256SUMS")}
        output = self.helper("Install", "-InPlace", "-Check")
        self.assertIn("Installation preflight passed", output)
        self.assertEqual(self.snapshot(), before)
        self.assertFalse(self.receipt.exists())
        for path, acl in acls.items():
            self.assertEqual(self.acl(path), acl)
        self.helper("Install", "-InPlace")
        registered = self.snapshot()
        self.helper("Install", "-InPlace", "-Check")
        self.assertEqual(self.snapshot(), registered)

    def test_untrusted_mutation_refuses_before_registration(self):
        for sid in ("S-1-5-11", "S-1-5-32-545", "S-1-1-0"):
            for path in (self.root, self.bundle / "bin/semwright-mcp.exe",
                         self.bundle / "SHA256SUMS"):
                with self.subTest(sid=sid, path=path.name):
                    original_acl = self.acl(path)
                    try:
                        self.acl(path, sid=sid, rights="Modify")
                        before = self.snapshot()
                        for args in (("-Check",), ()):
                            output = self.helper("Install", "-InPlace", *args, success=False)
                            self.assertIn("Untrusted Windows installation path", output)
                            self.assertIn(path.name, output)
                            self.assertFalse(self.receipt.exists())
                            self.assertEqual(self.snapshot(), before)
                    finally:
                        self.acl(path, restore=original_acl)

    def test_inherit_only_ancestor_mutation_does_not_trust_new_children(self):
        # A protected existing bundle can be trusted even when a parent
        # contains an inherit-only mutation template for FUTURE children.
        subprocess.run(
            ["icacls", str(self.bundle), "/inheritance:d"],
            env=PS_ENV, capture_output=True, check=True, timeout=15,
        )
        original_bundle = self.snapshot()
        subprocess.run(
            ["icacls", str(self.root), "/grant", "*S-1-5-11:(OI)(CI)(IO)M"],
            env=PS_ENV, capture_output=True, check=True, timeout=15,
        )
        self.helper("Install", "-InPlace", "-Check")
        self.assertFalse(self.receipt.exists())
        self.assertEqual(self.snapshot(), original_bundle)

        # A new sibling receives the broad mutation as effective access.
        # It must not gain admission merely because the existing bundle did.
        future_bundle = self.root / "Bundle created after template"
        shutil.copytree(self.bundle, future_bundle)
        future_before = {p.relative_to(future_bundle).as_posix(): p.read_bytes()
                         for p in future_bundle.rglob("*") if p.is_file()}
        output = self.helper(
            "Install", "-InPlace", "-Check", success=False, bundle=future_bundle,
        )
        self.assertIn("Untrusted Windows installation path", output)
        self.assertIn(future_bundle.name, output)
        self.assertFalse((future_bundle / ".semwright-install-receipt").exists())
        self.assertEqual(
            {p.relative_to(future_bundle).as_posix(): p.read_bytes()
             for p in future_bundle.rglob("*") if p.is_file()},
            future_before,
        )

    def test_parent_delete_child_refuses_before_registration(self):
        self.acl(self.root, sid="S-1-1-0", rights="DeleteSubdirectoriesAndFiles")
        before = self.snapshot()
        output = self.helper("Install", "-InPlace", success=False)
        self.assertIn("Untrusted Windows installation path", output)
        self.assertFalse(self.receipt.exists())
        self.assertEqual(self.snapshot(), before)

    def test_copy_preflight_rejects_private_data_read_inheritance(self):
        if not self.root.is_relative_to(Path.home()):
            self.skipTest("copy fixture must be under the actual user profile")
        parent = self.root / "destination"
        parent.mkdir()
        self.acl(parent, sid="S-1-5-32-545", rights="Read", template=True)
        prefix = parent / "new-install"
        for args in (("-Check",), ()):
            output = self.helper("Install", "-Prefix", prefix, *args, success=False)
            self.assertIn("Untrusted Windows installation path", output)
            self.assertFalse(prefix.exists())

    def test_public_executable_read_is_allowed_but_manifest_read_is_private(self):
        executable = self.bundle / "bin/semwright-mcp.exe"
        self.acl(executable, sid="S-1-5-32-545", rights="ReadAndExecute")
        self.helper("Install", "-InPlace", "-Check")
        manifest = self.bundle / "SHA256SUMS"
        self.acl(manifest, sid="S-1-5-32-545", rights="Read")
        before = self.snapshot()
        output = self.helper("Install", "-InPlace", success=False)
        self.assertIn("Untrusted Windows installation path", output)
        self.assertFalse(self.receipt.exists())
        self.assertEqual(self.snapshot(), before)

    def test_copy_preflight_does_not_create_destination(self):
        if not self.root.is_relative_to(Path.home()):
            self.skipTest("copy fixture must be under the actual user profile")
        prefix = self.root / "new" / "copied-install"
        self.helper("Install", "-Prefix", prefix, "-Check")
        self.assertFalse(prefix.parent.exists())
        self.helper("Install", "-Prefix", prefix)
        self.assertTrue((prefix / ".semwright-install-receipt").is_file())
        self.assertEqual(self.acl(prefix / "SHA256SUMS"),
                         self.acl(prefix / ".semwright-install-receipt"))

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

    def test_printed_commands_preserve_literal_installation_paths(self):
        self.assert_printed_commands("Owner's $cache; [portable]")

    def test_printed_commands_preserve_smart_quotes(self):
        self.assert_printed_commands("Owner\u2018s\u2019 data\u201a\u201b $cache; [portable]")

    def assert_printed_commands(self, name):
        renamed = self.root / name
        self.bundle.rename(renamed)
        self.bundle = renamed
        installed = subprocess.run(
            [SHELL, "-NoProfile", "-NonInteractive", "-Command",
             "[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false); "
             "& $env:SEMWRIGHT_INSTALL_SCRIPT -InPlace"],
            env={**PS_ENV, "SEMWRIGHT_INSTALL_SCRIPT": str(self.bundle / HELPERS[0])},
            capture_output=True, encoding="utf-8", timeout=20,
        )
        self.assertEqual(installed.returncode, 0, installed.stdout + installed.stderr)
        output = installed.stdout
        labels = ("Next: ", "Then start the broker and run: ", "Remove: ")
        commands = []
        for label in labels:
            matching = [line[len(label):] for line in output.splitlines()
                        if line.startswith(label)]
            self.assertEqual(len(matching), 1, output)
            commands.extend(matching)
        inputs = self.root / "printed-commands.json"
        inputs.write_text(json.dumps(commands), encoding="utf-8")
        # Parse only: never execute installer-provided command text. The fixture
        # executables are inert; the oracle is PowerShell's actual command AST.
        parser = r"""
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
$results = foreach ($text in (Get-Content -LiteralPath $env:SEMWRIGHT_PRINTED_COMMANDS -Raw | ConvertFrom-Json)) {
    $tokens = $null
    $parseErrors = $null
    $ast = [System.Management.Automation.Language.Parser]::ParseInput($text, [ref]$tokens, [ref]$parseErrors)
    if ($parseErrors.Count -ne 0) { throw 'Printed command is not valid PowerShell.' }
    $statements = @($ast.EndBlock.Statements)
    if ($statements.Count -ne 1) { throw 'Expected exactly one statement.' }
    $pipeline = @($statements[0].PipelineElements)
    if ($pipeline.Count -ne 1) { throw 'Expected exactly one command.' }
    $command = $pipeline[0]
    if ($command.InvocationOperator -ne 'Ampersand') { throw 'Expected direct invocation.' }
    $elements = @($command.CommandElements)
    if ($elements[0] -isnot [System.Management.Automation.Language.StringConstantExpressionAst]) {
        throw 'Executable path is not a literal.'
    }
    [pscustomobject]@{
        path = $elements[0].Value
        args = @($elements | Select-Object -Skip 1 | ForEach-Object { $_.Extent.Text })
    }
}
ConvertTo-Json -InputObject @($results) -Depth 4 -Compress
"""
        result = subprocess.run(
            [SHELL, "-NoProfile", "-NonInteractive", "-Command", parser],
            env={**PS_ENV, "SEMWRIGHT_PRINTED_COMMANDS": str(inputs)},
            capture_output=True, encoding="utf-8", timeout=15,
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        cli = str(self.bundle / "bin/semwright.exe")
        self.assertEqual(json.loads(result.stdout), [
            {"path": cli, "args": ["setup"]},
            {"path": cli, "args": ["--json", "doctor"]},
            {"path": str(self.bundle / "Uninstall-Semwright.ps1"), "args": []},
        ])

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

    def test_live_bundle_process_refuses_uninstall_before_any_deletion(self):
        # Run only a copied OS command interpreter in this test fixture.
        # No actual Semwright broker or application workspace is touched.
        original = shutil.which("cmd.exe")
        self.assertIsNotNone(original, "Windows command processor required")
        stub = self.bundle / "bin/semwrightd.exe"
        shutil.copyfile(original, stub)
        self.checksums()
        self.helper("Install", "-InPlace")
        process = subprocess.Popen(
            [str(stub), "/Q", "/K"],
            stdin=subprocess.PIPE,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            cwd=self.root,
            creationflags=subprocess.CREATE_NO_WINDOW,
        )
        try:
            time.sleep(0.2)
            self.assertIsNone(process.poll(), "Test-owned executable exited early")
            before = self.snapshot()
            output = self.helper("Uninstall", success=False)
            self.assertIn("Stop running Semwright from this bundle", output)
            self.assertEqual(self.snapshot(), before)
        finally:
            if process.poll() is None:
                process.terminate()
            process.wait(timeout=5)
            if process.stdin is not None:
                process.stdin.close()
        self.helper("Uninstall")
        self.assertFalse(self.bundle.exists())

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
        # No directory is created: the prefix gate must refuse even in check mode.
        outside = Path(self.root.anchor) / (self.root.name + '-outside-profile')
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
            env=PS_ENV, capture_output=True, check=True, timeout=15,
        )
        self.addCleanup(lambda: os.rmdir(alias) if alias.exists() else None)
        output = self.helper("Install", "-InPlace", success=False, bundle=alias)
        self.assertIn("Reparse point refused", output)
        self.assertFalse(self.receipt.exists())


if __name__ == "__main__":
    unittest.main()
