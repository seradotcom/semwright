#!/usr/bin/env python3
"""Build an exact-SHA source archive and consume it from a clean path."""
from __future__ import annotations

import hashlib
import json
import os
import subprocess
import tarfile
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def run(argv: list[str], *, cwd: Path, env: dict[str, str] | None = None) -> str:
    process = subprocess.run(
        argv,
        cwd=cwd,
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        timeout=900,
    )
    print("$", " ".join(argv), flush=True)
    print(process.stdout, end="", flush=True)
    if process.returncode:
        raise AssertionError(process.stdout)
    return process.stdout


class CleanRoomPackageTests(unittest.TestCase):
    def test_exact_sha_archive_builds_clean_consumers_after_extraction(self) -> None:
        if os.getenv("GITHUB_ACTIONS") != "true":
            self.fail("package clean-room is GitHub Actions-only final evidence")
        sha = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
        expected = os.environ.get("EXPECTED_SHA")
        self.assertEqual(expected, sha)
        tree = subprocess.check_output(["git", "rev-parse", "HEAD^{tree}"], cwd=ROOT, text=True).strip()

        out = ROOT / "verification/native-sdk/package"
        out.mkdir(parents=True, exist_ok=True)
        archive = out / f"semwright-native-sdk-{sha}.tar"
        prefix = f"semwright-native-sdk-{sha}/"
        with archive.open("wb") as handle:
            process = subprocess.run(
                ["git", "archive", "--format=tar", f"--prefix={prefix}", sha],
                cwd=ROOT,
                stdout=handle,
                stderr=subprocess.PIPE,
                timeout=120,
            )
        self.assertEqual(process.returncode, 0, process.stderr.decode("utf-8", "replace"))
        archive_sha = hashlib.sha256(archive.read_bytes()).hexdigest()

        with tarfile.open(archive, "r:") as tf:
            members = tf.getmembers()
            names = [m.name for m in members]
            self.assertTrue(any(n.endswith("/crates/native-sdk/Cargo.toml") for n in names))
            self.assertTrue(any(n.endswith("/sdk/native-typescript/package.json") for n in names))
            self.assertTrue(any(n.endswith("/docs/native-sdk/MIGRATION_FROM_0_3.md") for n in names))
            self.assertFalse(any(Path(n).is_absolute() or ".." in Path(n).parts for n in names))

        manifest = {
            "schema_version": 1,
            "source_sha": sha,
            "source_tree": tree,
            "archive": archive.name,
            "archive_sha256": archive_sha,
            "tracked_members": len(members),
            "license": "MIT OR Apache-2.0",
            "purpose": "exact-SHA clean-room validation",
        }
        (out / "PACKAGE.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
        (out / f"{archive.name}.sha256").write_text(
            f"{archive_sha}  {archive.name}\n", encoding="utf-8"
        )

        with tempfile.TemporaryDirectory(prefix="semwright-native-package-") as raw:
            destination = Path(raw) / "clean room with spaces Ω"
            destination.mkdir()
            with tarfile.open(archive, "r:") as tf:
                tf.extractall(destination, filter="data")
            extracted = destination / prefix.rstrip("/")
            self.assertTrue((extracted / "Cargo.toml").is_file())

            ts = extracted / "sdk/native-typescript"
            run(["npm", "ci", "--ignore-scripts", "--no-audit", "--no-fund"], cwd=ts)
            run(["npm", "run", "build"], cwd=ts)

            env = os.environ.copy()
            env["SEMWRIGHT_NATIVE_SOURCE_ROOT"] = str(extracted)
            env["CARGO_INCREMENTAL"] = "0"
            run(
                [os.environ.get("PYTHON", "python3"), "scripts/native-sdk/clean_consumers.py"],
                cwd=extracted,
                env=env,
            )

            self.assertNotIn(str(ROOT), json.dumps(manifest))
            self.assertEqual(hashlib.sha256(archive.read_bytes()).hexdigest(), archive_sha)


if __name__ == "__main__":
    unittest.main(verbosity=2)
