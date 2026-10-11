"""Exercise approval startup through the real daemon, without a desktop session."""

import os
from pathlib import Path
import subprocess
import sys
import unittest
import uuid


@unittest.skipUnless(os.name == "nt", "Windows console admission contract")
class WindowsApprovalConsoleTests(unittest.TestCase):
    def test_detached_broker_cannot_use_redirected_input_for_approval(self):
        self.assert_unavailable_console(subprocess.DETACHED_PROCESS)

    def test_windowless_console_cannot_offer_human_approval(self):
        self.assert_unavailable_console(subprocess.CREATE_NO_WINDOW)

    def assert_unavailable_console(self, creationflags):
        daemon = Path(os.environ.get("SEMWRIGHT_TEST_DAEMON", "target/debug/semwrightd.exe"))
        self.assertTrue(daemon.is_file(), f"Build the daemon before running this test: {daemon}")
        result = subprocess.run(
            [str(daemon.resolve()), "--fake", "--approval-console", "--socket",
             rf"\\.\pipe\semwright-console-test-{uuid.uuid4().hex}"],
            input="approve attacker-controlled-input\n",
            text=True,
            capture_output=True,
            creationflags=creationflags,
            timeout=15,
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("ConsentRequired", result.stderr)
        self.assertIn("controlling console", result.stderr)
        self.assertNotIn("not yet implemented", result.stderr)
        self.assertNotIn("broker ready", result.stderr)

    def test_private_console_approval_with_redirected_stdio(self):
        result = subprocess.run(
            [sys.executable, str(Path(__file__).resolve()), "--private-console-fixture"],
            capture_output=True, text=True, creationflags=subprocess.DETACHED_PROCESS, timeout=50,
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("private console approval PASS", result.stdout)


def private_console_fixture():
    """Synthetic input is confined to a new hidden console and a --fake broker."""
    import ctypes
    from ctypes import wintypes as w
    import json
    import re
    import tempfile
    import time

    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel.AllocConsole.restype = w.BOOL
    if not kernel.AllocConsole():
        raise RuntimeError("Fixture must start detached and allocate its own console")
    kernel.GetConsoleWindow.restype = w.HWND
    user = ctypes.WinDLL("user32", use_last_error=True)
    user.ShowWindow.argtypes = [w.HWND, ctypes.c_int]
    user.ShowWindow(kernel.GetConsoleWindow(), 0)
    kernel.CreateFileW.argtypes = [w.LPCWSTR, w.DWORD, w.DWORD, w.LPVOID, w.DWORD, w.DWORD, w.HANDLE]
    kernel.CreateFileW.restype = w.HANDLE
    output = kernel.CreateFileW("CONOUT$", 0xC0000000, 3, None, 3, 0, None)
    input_handle = kernel.CreateFileW("CONIN$", 0xC0000000, 3, None, 3, 0, None)

    class Coord(ctypes.Structure):
        _fields_ = [("X", w.SHORT), ("Y", w.SHORT)]

    class SmallRect(ctypes.Structure):
        _fields_ = [("Left", w.SHORT), ("Top", w.SHORT), ("Right", w.SHORT), ("Bottom", w.SHORT)]

    class ScreenInfo(ctypes.Structure):
        _fields_ = [("size", Coord), ("cursor", Coord), ("attributes", w.WORD), ("window", SmallRect), ("maximum", Coord)]

    class Key(ctypes.Structure):
        _fields_ = [("down", w.BOOL), ("repeat", w.WORD), ("virtual", w.WORD), ("scan", w.WORD), ("character", w.WORD), ("control", w.DWORD)]

    class Event(ctypes.Union):
        _fields_ = [("key", Key), ("padding", ctypes.c_byte * 16)]

    class Record(ctypes.Structure):
        _fields_ = [("kind", w.WORD), ("event", Event)]

    kernel.GetConsoleScreenBufferInfo.argtypes = [w.HANDLE, ctypes.POINTER(ScreenInfo)]
    kernel.ReadConsoleOutputCharacterW.argtypes = [w.HANDLE, w.LPWSTR, w.DWORD, Coord, ctypes.POINTER(w.DWORD)]
    kernel.WriteConsoleInputW.argtypes = [w.HANDLE, ctypes.POINTER(Record), w.DWORD, ctypes.POINTER(w.DWORD)]

    def screen():
        info = ScreenInfo()
        if not kernel.GetConsoleScreenBufferInfo(output, ctypes.byref(info)):
            raise ctypes.WinError(ctypes.get_last_error())
        length = min(info.size.X * info.size.Y, 65536)
        buffer = ctypes.create_unicode_buffer(length)
        count = w.DWORD()
        if not kernel.ReadConsoleOutputCharacterW(output, buffer, length, Coord(0, 0), ctypes.byref(count)):
            raise ctypes.WinError(ctypes.get_last_error())
        return buffer[:count.value]

    def type_fixture(text):
        records = (Record * len(text))()
        for record, character in zip(records, text):
            record.kind = 1
            record.event.key.down = 1
            record.event.key.repeat = 1
            record.event.key.character = ord(character)
        count = w.DWORD()
        if not kernel.WriteConsoleInputW(input_handle, records, len(records), ctypes.byref(count)) or count.value != len(records):
            raise ctypes.WinError(ctypes.get_last_error())

    daemon = Path(os.environ.get("SEMWRIGHT_TEST_DAEMON", "target/debug/semwrightd.exe")).resolve()
    cli = Path(os.environ.get("SEMWRIGHT_TEST_CLI", "target/debug/semwright.exe")).resolve()
    if not daemon.is_file() or not cli.is_file():
        raise RuntimeError("Build the daemon and CLI before the console fixture")
    broker = None
    requests = []
    seen = set()
    def next_challenge(request, label):
        deadline = time.monotonic() + 5
        while True:
            challenges = set(re.findall(r"Type exactly: approve ([0-9a-f]{12})", screen())) - seen
            if challenges:
                challenge = challenges.pop()
                seen.add(challenge)
                return challenge
            if request.poll() is not None or time.monotonic() >= deadline:
                raise RuntimeError("Approval prompt missing for " + label)
            time.sleep(0.02)

    try:
        with tempfile.TemporaryDirectory(prefix="semwright-console-") as directory:
            config = Path(directory) / "daemon.toml"
            config.write_text('[policy]\nprofile = "observe"\nallow = ["clipboard.write", "clipboard.read"]\nconfirm_mutations = true\n', encoding="utf-8")
            socket = rf"\\.\pipe\semwright-console-{uuid.uuid4().hex}"
            broker = subprocess.Popen(
                [str(daemon), "--fake", "--config", str(config), "--socket", socket, "--approval-console"],
                stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
            )
            deadline = time.monotonic() + 10
            while True:
                if broker.poll() is not None:
                    raise RuntimeError("Broker startup failed: " + broker.communicate()[1])
                probe = subprocess.run([str(cli), "--socket", socket, "--json", "doctor"], capture_output=True, timeout=3)
                if probe.returncode == 0:
                    break
                if time.monotonic() >= deadline:
                    raise RuntimeError("Broker readiness timed out")
                time.sleep(0.05)

            for mode in ("cancel", "whitespace", "unicode", "long", "approve", "replay"):
                request = subprocess.Popen(
                    [str(cli), "--socket", socket, "--json", "execute", "clipboard.write", "--args-json", '{"text":"synthetic console fixture"}'],
                    stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
                )
                requests.append(request)
                previous = next(iter(seen), "000000000000")
                challenge = next_challenge(request, mode)
                # Redirected stdin must never become an affirmative approval source.
                broker.stdin.write(f"approve {challenge}\n")
                broker.stdin.flush()
                time.sleep(0.05)
                if request.poll() is not None:
                    raise RuntimeError("Redirected stdin supplied approval")
                if mode == "cancel":
                    request.kill()
                    request.communicate(timeout=3)
                    # The read has its own SecretAccess approval. Its prompt proves the
                    # cancelled request released the Broker I/O lock.
                    read = subprocess.Popen(
                        [str(cli), "--socket", socket, "--json", "execute", "clipboard.read"],
                        stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
                    )
                    requests.append(read)
                    read_challenge = next_challenge(read, "read after cancellation")
                    type_fixture(f"approve {read_challenge}\r")
                    read_stdout, read_stderr = read.communicate(timeout=3)
                    value = json.loads(read_stdout)
                    if not value["ok"] or value["data"]["text"] == "synthetic console fixture":
                        raise RuntimeError("Cancelled approval dispatched a mutation")
                    # Queue a stale line after cancellation. The next prompt must discard it.
                    type_fixture(f"approve {challenge}\r")
                    continue
                answer = {
                    "whitespace": f" approve {challenge}\r",
                    "unicode": "\u00e9\r",
                    "long": "x" * 257 + "\r",
                    "approve": f"approve {challenge}\r",
                    "replay": f"approve {previous}\r",
                }[mode]
                type_fixture(answer)
                stdout, stderr = request.communicate(timeout=5)
                envelope = json.loads(stdout)
                if bool(envelope["ok"]) != (mode == "approve"):
                    raise RuntimeError(f"Wrong approval result for {mode}: {stdout} {stderr}")
            print("private console approval PASS")
    finally:
        for request in requests:
            if request.poll() is None:
                request.kill()
            request.communicate()
        if broker is not None:
            if broker.poll() is None:
                broker.kill()
            broker.communicate()
        kernel.CloseHandle.argtypes = [w.HANDLE]
        kernel.CloseHandle(input_handle)
        kernel.CloseHandle(output)
        kernel.FreeConsole()


if __name__ == "__main__":
    if "--private-console-fixture" in sys.argv:
        private_console_fixture()
    else:
        unittest.main()
