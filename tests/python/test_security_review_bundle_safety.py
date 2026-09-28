"""Destructive-output regressions run only against disposable Git repositories."""
from pathlib import Path
import hashlib
import json
import os
import subprocess
import tarfile
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts/dev/security-review-bundle.sh"
REQUIRED = (
    "docs/security-review.md", "docs/requirements/SECURITY_THREAT_MODEL.md",
    "RELEASE_BLOCKERS.md", "VERIFY.md", "SECURITY.md",
    ".github/workflows/security.yml", "Cargo.lock", "deny.toml",
)


@unittest.skipUnless(os.name == "posix", "bundle uses the documented POSIX toolchain")
class SecurityReviewBundleSafetyTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="semwright-bundle-test-")
        self.addCleanup(self.tmp.cleanup)
        self.parent = Path(self.tmp.name)
        self.repo = self.parent / "repo"
        self.repo.mkdir()
        self.git("init", "-q")
        for name in REQUIRED:
            path = self.repo / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(f"Disposable fixture: {name}\n")
        self.git("add", ".")
        self.git("-c", "user.name=Semwright Fixture", "-c",
                 "user.email=fixture@example.invalid", "-c", "commit.gpgsign=false",
                 "-c", "core.hooksPath=/dev/null", "commit", "-qm", "fixture")
        self.sha = self.git("rev-parse", "HEAD").strip()

    def git(self, *args):
        return subprocess.check_output(["git", *args], cwd=self.repo, text=True)

    def bundle(self, output, baseline="HEAD"):
        return subprocess.run([str(SCRIPT), baseline, str(output)], cwd=self.repo,
                              text=True, capture_output=True, timeout=20, umask=0)

    def test_existing_directory_is_preserved(self):
        output = self.parent / "existing"
        output.mkdir()
        sentinel = output / "evidence.txt"
        sentinel.write_text("must survive")
        result = self.bundle(output)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(sentinel.read_text(), "must survive")
        self.assertEqual(sorted(p.name for p in output.iterdir()), ["evidence.txt"])

    def test_existing_file_is_preserved(self):
        output = self.parent / "existing-file"
        output.write_text("must survive")
        self.assertNotEqual(self.bundle(output).returncode, 0)
        self.assertEqual(output.read_text(), "must survive")

    def test_existing_and_dangling_symlinks_are_preserved(self):
        target = self.parent / "target"
        target.mkdir()
        (target / "sentinel").write_text("must survive")
        for name, destination in (("link", target), ("dangling", self.parent / "absent")):
            with self.subTest(name=name):
                output = self.parent / name
                output.symlink_to(destination, target_is_directory=True)
                self.assertNotEqual(self.bundle(output).returncode, 0)
                self.assertTrue(output.is_symlink())
        self.assertEqual((target / "sentinel").read_text(), "must survive")
        self.assertFalse((self.parent / "absent").exists())

    def test_worktree_and_git_metadata_are_not_output_destinations(self):
        for output in (self.repo, self.repo / "new-packet", self.repo / ".git/new-packet"):
            with self.subTest(output=output):
                self.assertNotEqual(self.bundle(output).returncode, 0)
                self.assertEqual(self.git("rev-parse", "HEAD").strip(), self.sha)
                self.assertFalse((self.repo / "new-packet").exists())
                self.assertFalse((self.repo / ".git/new-packet").exists())

    def test_invalid_baseline_does_not_create_output(self):
        for baseline in ("no-such-revision", "--help"):
            with self.subTest(baseline=baseline):
                output = self.parent / "invalid-baseline"
                self.assertNotEqual(self.bundle(output, baseline).returncode, 0)
                self.assertFalse(output.exists())

    def test_missing_reference_does_not_create_output(self):
        self.git("rm", "--", "SECURITY.md")
        self.git("-c", "user.name=Semwright Fixture", "-c",
                 "user.email=fixture@example.invalid", "-c", "commit.gpgsign=false",
                 "-c", "core.hooksPath=/dev/null", "commit", "-qm", "missing reference")
        output = self.parent / "incomplete"
        self.assertNotEqual(self.bundle(output).returncode, 0)
        self.assertFalse(output.exists())

    def test_control_characters_and_missing_parent_are_rejected(self):
        for output in (self.parent / "bad\nname", self.parent / "absent/packet"):
            with self.subTest(output=output):
                self.assertNotEqual(self.bundle(output).returncode, 0)
                self.assertFalse(output.exists())

    def test_symlinked_parent_cannot_enter_worktree(self):
        alias = self.parent / "alias"
        alias.symlink_to(self.repo, target_is_directory=True)
        self.assertNotEqual(self.bundle(alias / "packet").returncode, 0)
        self.assertFalse((self.repo / "packet").exists())

    def test_success_is_private_commit_scoped_and_unreviewed(self):
        (self.repo / "untracked.txt").write_text("not part of the snapshot")
        (self.repo / "SECURITY.md").write_text("uncommitted working-tree change")
        output = self.parent / "new packet with spaces"
        result = self.bundle(output)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(output.stat().st_mode & 0o777, 0o700)
        manifest = json.loads((output / "manifest.json").read_text())
        self.assertEqual(manifest["baseline_sha"], self.sha)
        self.assertEqual(manifest["status"], "UNREVIEWED")
        self.assertTrue(manifest["independent_review_required"])
        self.assertFalse(manifest["self_attestation"])
        self.assertEqual((output / "BASELINE_SHA").read_text().strip(), self.sha)
        self.assertIn("BASELINE_SHA", (output / "SHA256SUMS").read_text())
        for path in output.rglob("*"):
            self.assertEqual(path.stat().st_mode & 0o077, 0, str(path))
        check = subprocess.run(["sha256sum", "-c", "SHA256SUMS"], cwd=output,
                               text=True, capture_output=True, timeout=10)
        self.assertEqual(check.returncode, 0, check.stderr)
        archive = output / manifest["source_snapshot"]["archive"]
        self.assertEqual(hashlib.sha256(archive.read_bytes()).hexdigest(),
                         manifest["source_snapshot"]["sha256"])
        with tarfile.open(archive) as tar:
            self.assertFalse(any(n.endswith("/untracked.txt") for n in tar.getnames()))
        self.assertEqual((output / "reference/SECURITY.md").read_text(),
                         "Disposable fixture: SECURITY.md\n")

    def test_second_generation_cannot_replace_a_hashed_bundle(self):
        output = self.parent / "immutable"
        first = self.bundle(output)
        self.assertEqual(first.returncode, 0, first.stderr)
        before = {str(p.relative_to(output)): p.read_bytes()
                  for p in output.rglob("*") if p.is_file()}
        self.assertNotEqual(self.bundle(output).returncode, 0)
        after = {str(p.relative_to(output)): p.read_bytes()
                 for p in output.rglob("*") if p.is_file()}
        self.assertEqual(after, before)

    def test_baseline_file_tampering_is_detected(self):
        output = self.parent / "tamper-check"
        result = self.bundle(output)
        self.assertEqual(result.returncode, 0, result.stderr)
        (output / "BASELINE_SHA").write_text("0" * 40 + "\n")
        check = subprocess.run(["sha256sum", "-c", "SHA256SUMS"], cwd=output,
                               text=True, capture_output=True, timeout=10)
        self.assertNotEqual(check.returncode, 0)
        self.assertIn("BASELINE_SHA: FAILED", check.stdout)

    def test_source_archive_is_reproducible(self):
        digests = []
        for name in ("first", "second"):
            output = self.parent / name
            result = self.bundle(output)
            self.assertEqual(result.returncode, 0, result.stderr)
            manifest = json.loads((output / "manifest.json").read_text())
            digests.append(manifest["source_snapshot"]["sha256"])
        self.assertEqual(digests[0], digests[1])


if __name__ == "__main__":
    unittest.main()
