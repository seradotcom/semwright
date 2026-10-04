#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
import zipfile

HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("composition_package_dev", HERE / "package-dev.py")
assert SPEC and SPEC.loader
package = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(package)


class DevelopmentPackageTests(unittest.TestCase):
    def test_deterministic_combined_bundle_verifies_source_lineage(self):
        with tempfile.TemporaryDirectory(prefix="composition-package-test-") as temp:
            root = Path(temp)
            first = root / "first.zip"
            second = root / "second.zip"
            kwargs = {
                "root": package.ROOT,
                "explicit_sha": "a" * 40,
                "explicit_epoch": 1_800_000_000,
            }
            first_result = package.build(first, **kwargs)
            second_result = package.build(second, **kwargs)
            self.assertEqual(first_result["sha256"], second_result["sha256"])

            verified = package.verify(first)
            self.assertTrue(verified["valid"])
            self.assertEqual(verified["source_sha"], "a" * 40)
            self.assertEqual(verified["audio_integration"], "certified-b-exact-sha")

            with zipfile.ZipFile(first) as archive:
                metadata = json.loads(archive.read(package.GENERATED_METADATA))
                self.assertEqual(metadata["scope"], "combined-candidate")
                self.assertEqual(
                    metadata["composition_source_sha"],
                    "16bc180a2cc6819787c805df5c391de9c11c985c",
                )
                self.assertEqual(
                    metadata["audio_source_sha"],
                    "8ed2d30c8ba797ebd5b8c102d34f8ab5bb3a28b3",
                )
                paths = {entry["path"] for entry in metadata["entries"]}
                for required in [
                    "skills/semwright-video-production/SKILL.md",
                    "skills/semwright-av-production/SKILL.md",
                    "skills/semwright-audio-production/SKILL.md",
                    "integrations/motion-canvas/runtime/package-lock.json",
                    "crates/driver-motion-canvas/src/composition.rs",
                    "crates/driver-figma/src/composition_kernel.rs",
                    "crates/driver-mlt-video/src/app.rs",
                    "crates/audio-authoring/src/lib.rs",
                    "crates/driver-faust-audio/src/driver.rs",
                    "crates/driver-ardour-audio/src/driver.rs",
                    ".github/workflows/composition-av-combined.yml",
                ]:
                    self.assertIn(required, paths)
                parts = {part for value in paths for part in Path(value).parts}
                self.assertNotIn("target", parts)
                self.assertNotIn("node_modules", parts)

    def test_archive_rejects_path_traversal(self):
        with tempfile.TemporaryDirectory(prefix="composition-package-hostile-") as temp:
            archive_path = Path(temp) / "hostile.zip"
            with zipfile.ZipFile(archive_path, "w") as archive:
                archive.writestr("../escape", b"bad")
                archive.writestr(package.GENERATED_METADATA, b"{}")
            with self.assertRaises(ValueError):
                package.verify(archive_path)


if __name__ == "__main__":
    unittest.main()
