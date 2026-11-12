"""Guard the shared Blender semantic/authoring implementations across native hosts.

Linux 4.5.14 and Windows 5.2.1 deliberately use different transports
(Unix socket versus verified native stdio). The domain meaning of their
semantic RNA and authoring substrate must not silently diverge.
"""
from __future__ import annotations

import ast
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[3]
SOURCE = ROOT / "crates/driver-blender/src"
WINDOWS = ROOT / "crates/driver-host/src/windows_blender52_runtime"


def normalized(source: bytes) -> bytes:
    return source.replace(b"\r\n", b"\n")


class NativeBlenderCrossVersionParity(unittest.TestCase):
    def test_rna_semantics_are_identical_for_linux_and_windows(self) -> None:
        linux = (SOURCE / "semantic.py").read_bytes()
        windows = (WINDOWS / "semantic.py").read_bytes()
        self.assertGreater(len(linux), 100_000)
        self.assertEqual(linux, windows)
        ast.parse(linux)
        ast.parse(windows)

    def test_native_authoring_domain_logic_is_identical(self) -> None:
        linux = normalized((SOURCE / "authoring_native.py").read_bytes())
        windows = normalized((WINDOWS / "authoring_native.py").read_bytes())
        self.assertGreater(len(linux), 50_000)
        self.assertEqual(linux, windows)
        ast.parse(linux)
        ast.parse(windows)

    def test_platform_transports_remain_separate(self) -> None:
        linux = (SOURCE / "bridge.py").read_text(encoding="utf-8")
        windows = (WINDOWS / "bridge.py").read_text(encoding="utf-8")
        self.assertIn("socket.AF_UNIX", linux)
        self.assertIn("stdio:", windows)
        self.assertIn("SEMWRIGHT-BLENDER-STDIO-v1:", windows)


if __name__ == "__main__":
    unittest.main()
