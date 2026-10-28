"""A6 AV27 typed original selector — inert, bounded, no native commands."""
from __future__ import annotations
import importlib.util
import json
import pathlib
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))
import ci_av_originals_egress as selector
P = "av-motion-" + "a"*16 + ".mkv"
Q = "av-motion-" + "b"*16 + ".mkv"

class Av27SelectorTests(unittest.TestCase):
    def setUp(self):
        self.d=tempfile.TemporaryDirectory()
        self.addCleanup(self.d.cleanup)
        self.root=pathlib.Path(self.d.name)/"private"
        self.root.mkdir()
        self.shared=self.root/"shared-output"
        self.shared.mkdir()
        self.patch=patch.object(selector, "directory", lambda *a, **k: None)
        self.patch.start()
        self.addCleanup(self.patch.stop)
    def add(self, suffix, data=b"real bytes", prefix=P):
        f=self.shared/(prefix+suffix)
        f.write_bytes(data)
        return f
    def test_exact_typed_allowlist(self):
        self.assertEqual(len(selector.LIMITS), 27)
        self.assertEqual(sum(selector.LIMITS.values()),1966080)
        self.assertEqual(selector.LIMITS[".ffv1-host.stdin.original.bin"],65536)
        self.assertEqual(selector.LIMITS[".ffv1-host.stdout.original.bin"],262144)
        self.assertEqual(selector.LIMITS[".ffv1-decode.terminal.original.json"],65536)
    def test_no_root_not_success(self):
        a=selector.inspect(self.root/"absent")
        self.assertEqual(a[2],"ROOT_ABSENT")
        self.assertIsNone(a[1])
    def test_no_shared_not_success(self):
        self.shared.rmdir()
        self.assertEqual(selector.inspect(self.root)[2],"SHARED_ABSENT")
    def test_no_originals_not_success(self):
        a=selector.inspect(self.root)
        self.assertEqual(a[0],[])
        self.assertIsNone(a[1])
    def test_selected_bytes_unchanged(self):
        f=self.add(".ffv1-host.stdin.original.bin",b"\0abc\r\n")
        a=selector.inspect(self.root)
        self.assertEqual(a[0][0][0],f)
        self.assertEqual(f.read_bytes(),b"\0abc\r\n")
    def test_zero_byte_genuine_original_kept(self):
        self.add(".ffv1-host.stdout.original.bin",b"")
        self.assertEqual(len(selector.inspect(self.root)[0]),1)
    def test_other_digest_rejected(self):
        self.add(".ffv1-host.stdin.original.bin")
        self.add(".ffv1-catalog.stdin.original.bin",prefix=Q)
        with self.assertRaisesRegex(Exception,"digest_or_file_budget"):
            selector.inspect(self.root)
    def test_exact_limit_allowed(self):
        self.add(".ffv1-encode.stdout.original.bin",b"x"*16384)
        self.assertEqual(len(selector.inspect(self.root)[0]),1)
    def test_excess_limit_denied(self):
        self.add(".ffv1-encode.stdout.original.bin",b"x"*16385)
        with self.assertRaisesRegex(Exception,"original_too_large"):
            selector.inspect(self.root)
    def test_media_not_selected(self):
        self.add("",b"MKV original is not a host command original")
        self.assertEqual(selector.inspect(self.root)[3],1)
    def test_argv_sidecar_not_selected(self):
        self.add(".ffv1-host.argv.original.bin")
        self.assertEqual(selector.inspect(self.root)[3],1)
    def test_unapproved_original_not_selected(self):
        self.add(".ffv1-private-key.stdout.original.bin")
        self.assertEqual(selector.inspect(self.root)[3],1)
    def test_extension_suffix_not_selected(self):
        self.add(".ffv1-host.stdout.original.bin.copy")
        self.assertEqual(selector.inspect(self.root)[3],1)
    def test_mismatched_terminal_extension_denied(self):
        self.add(".ffv1-host.terminal.original.bin")
        self.assertEqual(selector.inspect(self.root)[3],1)
    def test_directory_named_like_original_rejected(self):
        (self.shared/(P+".ffv1-host.stdout.original.bin")).mkdir()
        with self.assertRaisesRegex(Exception,"original_not_regular"):
            selector.inspect(self.root)
    def test_symlink_named_like_original_rejected(self):
        f=self.shared/"inert";f.write_bytes(b"data")
        try:
            (self.shared/(P+".ffv1-host.stdout.original.bin")).symlink_to(f)
        except (OSError,NotImplementedError):
            self.skipTest("symlink not available")
        with self.assertRaisesRegex(Exception,"original_not_regular"):
            selector.inspect(self.root)
    def test_non_original_namespace_limit(self):
        for n in range(257):
            (self.shared/f"sidecar-{n}").write_bytes(b"")
        with self.assertRaisesRegex(Exception,"namespace_budget"):
            selector.inspect(self.root)

if __name__ == "__main__":
    unittest.main()
