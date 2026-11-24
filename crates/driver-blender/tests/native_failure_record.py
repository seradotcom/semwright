"""Pure tests for bounded owner-local native exception records; no Blender process."""
import ast
import json
from types import SimpleNamespace
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from render_publication import source_modules


class NativeFailureRecordTests(unittest.TestCase):
    def each_source(self):
        for module in source_modules():
            with tempfile.TemporaryDirectory(prefix="semwright-native-diagnostic-") as directory:
                root = Path(directory).resolve()
                yield module, root, module.Commands(None, str(root))

    def test_retains_cause_and_frames_without_python_locals(self):
        for module, root, commands in self.each_source():
            secret_local = "must-never-serialize-frame-locals"
            try:
                try:
                    raise PermissionError(5, "bounded native permission failure", "artifact.glb")
                except PermissionError as cause:
                    raise RuntimeError("operator failed") from cause
            except RuntimeError as failure:
                self.assertTrue(commands.record_native_failure(failure))
            files = list(root.glob(".semwright-native-failure-*.json"))
            self.assertEqual(len(files), 1)
            raw = files[0].read_bytes()
            self.assertNotIn(secret_local.encode(), raw)
            record = json.loads(raw)
            self.assertEqual(record["schema"], 1)
            self.assertEqual([e["type"] for e in record["exceptions"]], ["RuntimeError", "PermissionError"])
            self.assertEqual(record["exceptions"][1]["errno"], 5)
            self.assertTrue(record["exceptions"][0]["frames"])
            self.assertLessEqual(len(raw), 8192)

    def test_bounds_multibyte_messages_and_retained_record_count(self):
        for module, root, commands in self.each_source():
            failure = RuntimeError("\U0001f30d" * 5000)
            for _ in range(8):
                self.assertTrue(commands.record_native_failure(failure))
            before = {p.name: p.read_bytes() for p in root.iterdir()}
            self.assertFalse(commands.record_native_failure(failure))
            self.assertEqual(before, {p.name: p.read_bytes() for p in root.iterdir()})
            for raw in before.values():
                self.assertLessEqual(len(raw), 8192)
                message = json.loads(raw)["exceptions"][0]["message"]
                self.assertLessEqual(len(message.encode("utf-8")), 1024)

    def test_existing_path_is_never_replaced(self):
        for module, root, commands in self.each_source():
            target = root / (".semwright-native-failure-" + "00" * 16 + ".json")
            target.write_bytes(b"owner-file")
            with patch.object(module.os, "urandom", return_value=b"\0" * 16):
                self.assertFalse(commands.record_native_failure(RuntimeError("later error")))
            self.assertEqual(target.read_bytes(), b"owner-file")
            self.assertEqual(len(list(root.iterdir())), 1)

    def test_storage_failure_does_not_replace_original_error(self):
        for module, root, commands in self.each_source():
            failure = RuntimeError("original native failure")
            with patch.object(module.os, "open", side_effect=PermissionError("storage denied")):
                self.assertFalse(commands.record_native_failure(failure))
            self.assertEqual(str(failure), "original native failure")
            self.assertEqual(list(root.iterdir()), [])

    def test_changed_workspace_identity_is_not_used_for_diagnostics(self):
        for module, root, commands in self.each_source():
            with patch.object(module.Path, "resolve", return_value=root.parent):
                self.assertFalse(commands.record_native_failure(RuntimeError("failed")))
            self.assertEqual(list(root.iterdir()), [])


class NativeStdioFailureTests(unittest.TestCase):
    def test_closed_stdio_retains_native_failures_but_never_exposes_details(self):
        bridge = Path(__file__).resolve().parents[2] / "driver-host/src/windows_blender52_runtime/bridge.py"
        tree = ast.parse(bridge.read_text())
        loop = compile(ast.Module(body=[tree.body[-1]], type_ignores=[]), str(bridge), "exec")
        for module in source_modules():
            for kind in ("unexpected", "backend", "policy"):
                with self.subTest(source=module.__name__, kind=kind), tempfile.TemporaryDirectory() as directory:
                    root = Path(directory).resolve()
                    commands = module.Commands(None, str(root))
                    error = (RuntimeError("private-native-detail") if kind == "unexpected" else
                             module.CommandError("BackendFailed" if kind == "backend" else "PolicyDenied",
                                                 "private-native-detail"))
                    frames = []
                    requests = iter([{"command": "driver.blender.export.glb", "args": {}}])
                    def reader(stream):
                        try:
                            return next(requests)
                        except StopIteration:
                            raise EOFError
                    def dispatch(command, args):
                        raise error
                    scope = {"_stdio": SimpleNamespace(sendall=lambda value: None), "nonce": "a" * 32,
                             "read_frame": reader, "write_frame": lambda stream, frame: frames.append(frame),
                             "dispatch": dispatch, "CommandError": module.CommandError,
                             "SemanticError": type("SemanticError", (Exception,), {}), "commands": commands}
                    if kind == "unexpected":
                        with self.assertRaises(SystemExit) as stopped:
                            exec(loop, scope)
                        self.assertEqual(stopped.exception.code, 3)
                    else:
                        exec(loop, scope)
                    code = {"unexpected": "NativeTransportFailed", "backend": "BackendFailed", "policy": "PolicyDenied"}[kind]
                    self.assertEqual(frames, [{"ok": False, "error": {"code": code}}])
                    records = list(root.glob(".semwright-native-failure-*.json"))
                    self.assertEqual(len(records), 0 if kind == "policy" else 1)


if __name__ == "__main__":
    unittest.main(verbosity=2)
