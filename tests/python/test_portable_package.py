"""Portable packaging regression tests that do not require native target binaries."""
import importlib.util
import os
from pathlib import Path
import stat
import struct
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location(
    "portable_package", ROOT / "scripts/release/portable_package.py"
)
PORTABLE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PORTABLE)


class PortablePackageTests(unittest.TestCase):
    def test_normalize_tree_does_not_require_follow_symlinks_utime(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root / "bin").mkdir()
            executable = root / "bin" / "semwright.exe"
            executable.write_bytes(b"payload")
            real_utime = os.utime

            def windows_like_utime(path, times, *args, **kwargs):
                if "follow_symlinks" in kwargs:
                    raise NotImplementedError(
                        "utime: follow_symlinks unavailable on this platform"
                    )
                return real_utime(path, times, *args, **kwargs)

            with mock.patch.object(PORTABLE.os, "utime", side_effect=windows_like_utime):
                PORTABLE.normalize_tree(root, 1_700_000_000)

            self.assertTrue(executable.is_file())

    @unittest.skipIf(os.name == "nt", "POSIX mode bits are not authoritative on Windows")
    def test_private_owner_file_is_mode_0600(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "policy.toml"
            PORTABLE.write_private_owner_file(path, '[policy]\nprofile="desktop"\n')
            self.assertEqual(stat.S_IMODE(path.stat().st_mode), 0o600)

    def test_normalize_tree_rejects_symlinks(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            target = root / "target"
            target.write_text("x")
            link = root / "link"
            try:
                link.symlink_to(target)
            except (OSError, NotImplementedError):
                self.skipTest("symlinks unavailable on this test host")
            with self.assertRaisesRegex(ValueError, "cannot contain symlinks"):
                PORTABLE.normalize_tree(root, 1_700_000_000)

    def test_parse_macos_rpath_dependencies(self):
        output = """/tmp/semwrightd:
	@rpath/libSemwrightNative.dylib (compatibility version 0.0.0, current version 0.0.0)
	/usr/lib/libSystem.B.dylib (compatibility version 1.0.0, current version 1351.0.0)
	@rpath/libOther.dylib (compatibility version 1.0.0, current version 1.0.0)
"""
        self.assertEqual(
            PORTABLE.parse_macos_rpath_dependencies(output),
            ("libOther.dylib", "libSemwrightNative.dylib"),
        )

    def test_parse_macos_rpath_rejects_parent_escape(self):
        output = """/tmp/semwrightd:
	@rpath/../evil.dylib (compatibility version 1.0.0, current version 1.0.0)
"""
        with self.assertRaisesRegex(ValueError, "unsafe @rpath dependency"):
            PORTABLE.parse_macos_rpath_dependencies(output)

    def test_find_macos_runtime_library_from_cargo_build_output(self):
        with tempfile.TemporaryDirectory() as folder:
            release = Path(folder)
            candidate = release / "build" / "native-abc" / "out" / "libSemwrightNative.dylib"
            candidate.parent.mkdir(parents=True)
            body = bytearray(128)
            body[:4] = b"\xcf\xfa\xed\xfe"
            struct.pack_into("<I", body, 4, PORTABLE.MACHO_CPU["arm64"])
            candidate.write_bytes(body)
            self.assertEqual(
                PORTABLE.find_macos_runtime_library(
                    release, "libSemwrightNative.dylib", "arm64"
                ),
                candidate,
            )


if __name__ == "__main__":
    unittest.main()
