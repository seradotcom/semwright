"""Actual Python add-on transport/dispatcher tests; not a live Blender certification."""
import json
import os
from pathlib import Path
import random
import socket
import stat
import struct
import sys
import tempfile
import threading
import time
import types
import unittest

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "adapters/blender"))
from semwright_blender.host import Server, Job, MAX_FRAME, read_frame, write_frame, private_directory
from semwright_blender.validation import CommandError, validate
from semwright_blender.commands import Commands, Workspace, SCHEMAS

class FramingTests(unittest.TestCase):
    def frame(self, raw):
        a, b = socket.socketpair()
        self.addCleanup(a.close)
        self.addCleanup(b.close)
        a.sendall(struct.pack(">I", len(raw)) + raw)
        a.shutdown(socket.SHUT_WR)
        return read_frame(b)

    def test_utf8_roundtrip(self):
        a, b = socket.socketpair()
        with a, b:
            value = {"text": "México 🦀", "ok": True, "list": [1, None]}
            write_frame(a, value)
            self.assertEqual(read_frame(b), value)

    def test_short_header(self):
        a, b = socket.socketpair()
        with a, b:
            a.sendall(b"\x00\x00")
            a.shutdown(socket.SHUT_WR)
            with self.assertRaises(EOFError):
                read_frame(b)

    def test_short_payload(self):
        a, b = socket.socketpair()
        with a, b:
            a.sendall(struct.pack(">I", 10) + b"{}")
            a.shutdown(socket.SHUT_WR)
            with self.assertRaises(EOFError):
                read_frame(b)

    def test_budget_checked_before_read(self):
        for length in (0, MAX_FRAME + 1, 2**32 - 1):
            with self.subTest(length=length):
                a, b = socket.socketpair()
                with a, b:
                    a.sendall(struct.pack(">I", length))
                    with self.assertRaises(ValueError):
                        read_frame(b)

    def test_duplicate_key_rejected(self):
        with self.assertRaises(ValueError):
            self.frame(b'{"x":1,"x":2}')

    def test_nonfinite_input_rejected(self):
        for raw in (b'{"x":NaN}', b'{"x":Infinity}', b'{"x":-Infinity}'):
            with self.subTest(raw=raw), self.assertRaises(ValueError):
                self.frame(raw)

    def test_nonfinite_output_rejected(self):
        a, b = socket.socketpair()
        with a, b, self.assertRaises(ValueError):
            write_frame(a, {"x": float("nan")})

    def test_seeded_roundtrips(self):
        randomizer = random.Random(20260921)
        for _ in range(250):
            value = {"n": randomizer.randrange(-(2**53), 2**53), "text": "abé🦀" * randomizer.randrange(60)}
            self.assertEqual(self.frame(json.dumps(value).encode()), value)


class ValidationTests(unittest.TestCase):
    def test_boolean_is_not_integer(self):
        with self.assertRaises(CommandError):
            validate(True, {"type": "integer"})

    def test_boolean_is_not_number(self):
        with self.assertRaises(CommandError):
            validate(False, {"type": "number"})

    def test_infinity_rejected(self):
        for value in (float("nan"), float("inf"), -float("inf")):
            with self.subTest(value=value), self.assertRaises(CommandError):
                validate(value, {"type": "number"})

    def test_unknown_field_rejected(self):
        with self.assertRaises(CommandError):
            validate({"python": "forbidden"}, SCHEMAS["blender.status"])

    def test_unlisted_primitive_rejected(self):
        with self.assertRaises(CommandError):
            validate({"name": "x", "primitive": "eval"}, SCHEMAS["blender.object.create"])

    def test_length_rejected(self):
        with self.assertRaises(CommandError):
            validate("abcd", {"type": "string", "maxLength": 3})

    def test_nul_rejected(self):
        with self.assertRaises(CommandError):
            validate("x\x00y", {"type": "string"})

    def test_open_object_schema_rejected(self):
        with self.assertRaises(CommandError):
            validate({}, {"type": "object"})

    def test_nesting_budget(self):
        with self.assertRaises(CommandError):
            validate(1, {"type": "integer"}, 17)

    def test_bounds(self):
        schema = {"type": "number", "minimum": 0, "maximum": 1}
        for value in (0, 0.1, 1):
            validate(value, schema)
        for value in (-0.1, 1.1):
            with self.assertRaises(CommandError):
                validate(value, schema)

    def test_array_bounds(self):
        schema = {"type": "array", "items": {"type": "integer"}, "minItems": 2, "maxItems": 2}
        validate([1, 2], schema)
        for value in ([], [1], [1, 2, 3], [1, "x"]):
            with self.assertRaises(CommandError):
                validate(value, schema)

    def test_every_host_schema_is_same_as_registry(self):
        registry = json.loads((ROOT / "schemas/commands.json").read_text())
        expected = {c["name"]: c["input_schema"] for c in registry if c["name"].startswith("blender.")}
        self.assertEqual(SCHEMAS, expected)


class WorkspaceTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.workspace = Workspace(self.root)

    def test_clean_new_path(self):
        self.assertEqual(self.workspace.path("scene.blend", ".blend"), str(self.root / "scene.blend"))

    def test_clean_existing_path(self):
        (self.root / "scene.blend").write_bytes(b"fixture")
        self.assertEqual(self.workspace.path("scene.blend", ".blend", True), str(self.root / "scene.blend"))

    def test_missing_existing_path(self):
        with self.assertRaises(CommandError) as caught:
            self.workspace.path("scene.blend", ".blend", True)
        self.assertEqual(caught.exception.code, "NotFound")

    def test_traversal_rejected(self):
        for path in ("../x.blend", "/x.blend", "a/../x.blend", "./x.blend", "a//x.blend", "\x00.blend", ""):
            with self.subTest(path=path), self.assertRaises(CommandError):
                self.workspace.path(path, ".blend")

    def test_suffix_rejected(self):
        with self.assertRaises(CommandError):
            self.workspace.path("x.py", ".blend")

    def test_leaf_symlink_rejected(self):
        (self.root / "link.blend").symlink_to("missing")
        with self.assertRaises(CommandError):
            self.workspace.path("link.blend", ".blend")

    def test_parent_symlink_rejected(self):
        (self.root / "real").mkdir()
        (self.root / "link").symlink_to("real", target_is_directory=True)
        with self.assertRaises(CommandError):
            self.workspace.path("link/x.blend", ".blend")

    def test_hardlink_rejected(self):
        (self.root / "a.blend").write_bytes(b"fixture")
        os.link(self.root / "a.blend", self.root / "b.blend")
        with self.assertRaises(CommandError):
            self.workspace.path("b.blend", ".blend")

    def test_fifo_rejected(self):
        os.mkfifo(self.root / "pipe.blend")
        with self.assertRaises(CommandError):
            self.workspace.path("pipe.blend", ".blend")

    def test_relative_root_rejected(self):
        with self.assertRaises(ValueError):
            Workspace("relative")

    def test_filesystem_root_rejected(self):
        with self.assertRaises(ValueError):
            Workspace("/")

    def test_private_directory_rejects_modes(self):
        target = self.root / "private"
        target.mkdir(mode=0o755)
        with self.assertRaises(PermissionError):
            private_directory(target)


class HostTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.path = Path(self.temp.name) / "bridge.sock"
        self.calls = []
        self.server = Server(self.path, self.dispatch)
        self.server.start()
        self.addCleanup(self.server.stop)

    def dispatch(self, command, args):
        self.calls.append((threading.get_ident(), command, args))
        if command == "raise":
            raise RuntimeError("SECRET must not escape via traceback")
        if command == "denied":
            raise CommandError("PolicyDenied", "SECRET details")
        return {"echo": args}

    def client(self, command="echo", args=None, identifier="a" * 32):
        connection = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        connection.settimeout(2)
        connection.connect(str(self.path))
        write_frame(connection, {"type": "hello", "protocol": 1})
        self.assertEqual(read_frame(connection)["type"], "ready")
        write_frame(connection, {"type": "execute", "id": identifier, "command": command, "args": args or {}})
        return connection

    def wait_queue(self):
        deadline = time.monotonic() + 2
        while self.server.jobs.empty() and time.monotonic() < deadline:
            time.sleep(0.005)
        self.assertFalse(self.server.jobs.empty())

    def test_socket_mode(self):
        self.assertEqual(stat.S_IMODE(self.path.stat().st_mode), 0o600)

    def test_main_thread_dispatch_only(self):
        with self.client(args={"text": "hello"}) as client:
            self.wait_queue()
            self.assertEqual(self.calls, [])
            self.server.drain()
            self.assertTrue(read_frame(client)["ok"])
        self.assertEqual(self.calls[0][0], threading.get_ident())

    def test_closed_connection_cancels_queued_work(self):
        client = self.client()
        self.wait_queue()
        job = self.server.jobs.queue[0]
        client.close()
        self.assertTrue(job.cancelled.wait(2))
        self.server.drain()
        self.assertEqual(self.calls, [])

    def test_expired_queued_job_does_not_dispatch(self):
        self.server.jobs.put(Job({"id": "a" * 32, "command": "echo", "args": {}}, time.monotonic() - 1))
        self.server.drain()
        self.assertEqual(self.calls, [])

    def test_exception_payload_redacted(self):
        with self.client("raise") as client:
            self.wait_queue()
            self.server.drain()
            result = read_frame(client)
        self.assertEqual(result["error"], {"code": "BackendFailed"})
        self.assertNotIn("SECRET", json.dumps(result))

    def test_declared_error_payload_redacted(self):
        with self.client("denied") as client:
            self.wait_queue()
            self.server.drain()
            result = read_frame(client)
        self.assertEqual(result["error"], {"code": "PolicyDenied"})
        self.assertNotIn("SECRET", json.dumps(result))

    def test_invalid_request_id_does_not_queue(self):
        with self.client(identifier="wrong") as client:
            self.assertEqual(client.recv(1), b"")
        self.assertTrue(self.server.jobs.empty())

    def test_wrong_handshake_closes(self):
        client = socket.socket(socket.AF_UNIX)
        self.addCleanup(client.close)
        client.settimeout(2)
        client.connect(str(self.path))
        write_frame(client, {"type": "hello", "protocol": 2})
        self.assertEqual(client.recv(1), b"")

    def test_existing_socket_not_replaced(self):
        other = Server(self.path, self.dispatch)
        with self.assertRaises(FileExistsError):
            other.start()

    def test_drain_budget(self):
        for _ in range(8):
            self.server.jobs.put(Job({"id": "a" * 32, "command": "echo", "args": {}}, time.monotonic() + 30))
        self.server.drain(max_jobs=3)
        self.assertEqual(len(self.calls), 3)
        self.assertEqual(self.server.jobs.qsize(), 5)


class Items:
    def __init__(self, factory):
        self.values = {}
        self.factory = factory
    def get(self, name):
        return self.values.get(name)
    def new(self, name, *args):
        value = self.factory(name)
        self.values[name] = value
        return value
    def remove(self, value, **kwargs):
        del self.values[value.name]
    def __iter__(self):
        return iter(self.values.values())
    def __len__(self):
        return len(self.values)
    def link(self, value):
        self.values[value.name] = value


def object_factory(name):
    return types.SimpleNamespace(name=name, type="EMPTY", location=[0, 0, 0], rotation_euler=[0, 0, 0], scale=[1, 1, 1], users_collection=[], data=types.SimpleNamespace(materials=[]), select_set=lambda value: None)


def mock_bpy():
    objects = Items(object_factory)
    collections = Items(lambda name: types.SimpleNamespace(name=name, objects=Items(object_factory)))
    def material(name):
        return types.SimpleNamespace(name=name, diffuse_color=[1, 1, 1, 1], roughness=0.5, metallic=0, use_nodes=False, node_tree=types.SimpleNamespace(nodes={}))
    materials = Items(material)
    render = types.SimpleNamespace(engine="CYCLES", resolution_x=64, resolution_y=64, resolution_percentage=100, filepath="", image_settings=types.SimpleNamespace(file_format="PNG"))
    scene = types.SimpleNamespace(name="Fixture", frame_current=1, objects=objects, camera=None, render=render, cycles=types.SimpleNamespace(samples=8), collection=types.SimpleNamespace(objects=objects, children=collections))
    bpy = types.SimpleNamespace(data=types.SimpleNamespace(objects=objects, collections=collections, materials=materials), context=types.SimpleNamespace(scene=scene, selected_objects=[], active_object=None, view_layer=types.SimpleNamespace(objects=types.SimpleNamespace(active=None))), app=types.SimpleNamespace(version=(4, 5, 0)), ops=types.SimpleNamespace())
    return bpy


class DispatcherTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.bpy = mock_bpy()
        self.commands = Commands(self.bpy, self.temp.name)

    def test_status(self):
        value = self.commands("blender.status", {})
        self.assertFalse(value["arbitrary_python"])
        self.assertEqual(value["protocol"], 1)

    def test_inspect(self):
        self.assertEqual(self.commands("blender.scene.inspect", {})["name"], "Fixture")

    def test_create_and_list_empty(self):
        self.commands("blender.object.create", {"name": "a", "primitive": "empty"})
        self.assertEqual(self.commands("blender.object.list", {})["objects"][0]["name"], "a")

    def test_collision_has_no_implicit_suffix(self):
        self.commands("blender.object.create", {"name": "a", "primitive": "empty"})
        with self.assertRaises(CommandError):
            self.commands("blender.object.create", {"name": "a", "primitive": "empty"})
        self.assertEqual(len(self.bpy.data.objects), 1)

    def test_transform(self):
        self.bpy.data.objects.new("a")
        result = self.commands("blender.object.transform", {"name": "a", "location": [1, 2, 3], "scale": [2, 2, 2]})
        self.assertEqual(result["object"]["location"], [1, 2, 3])
        self.assertEqual(result["object"]["scale"], [2, 2, 2])

    def test_delete(self):
        self.bpy.data.objects.new("a")
        self.commands("blender.object.delete", {"name": "a"})
        self.assertEqual(len(self.bpy.data.objects), 0)

    def test_collection_link_idempotent(self):
        self.bpy.data.objects.new("a")
        self.commands("blender.collection.create", {"name": "group"})
        self.assertTrue(self.commands("blender.collection.link", {"object": "a", "collection": "group"})["changed"])
        self.assertFalse(self.commands("blender.collection.link", {"object": "a", "collection": "group"})["changed"])

    def test_material_creation_and_assignment(self):
        self.bpy.data.objects.new("a")
        self.commands("blender.material.create", {"name": "m", "color": [1, 0, 0, 1], "roughness": 0.4})
        self.commands("blender.material.assign", {"object": "a", "material": "m"})
        self.assertEqual(self.bpy.data.objects.get("a").data.materials[0].name, "m")

    def test_unrestricted_python_command_absent(self):
        with self.assertRaises(CommandError) as caught:
            self.commands("blender.python.exec", {"code": "forbidden"})
        self.assertEqual(caught.exception.code, "Unsupported")

    def test_bad_transform_fails_before_change(self):
        obj = self.bpy.data.objects.new("a")
        with self.assertRaises(CommandError):
            self.commands("blender.object.transform", {"name": "a", "location": [True, 1, 2]})
        self.assertEqual(obj.location, [0, 0, 0])

    def test_open_disables_embedded_scripts(self):
        (Path(self.temp.name) / "safe.blend").write_bytes(b"fixture")
        calls = []
        self.bpy.ops.wm = types.SimpleNamespace(open_mainfile=lambda **kwargs: calls.append(kwargs) or {"FINISHED"})
        self.commands("blender.file.open", {"path": "safe.blend"})
        self.assertIs(calls[0]["use_scripts"], False)
        self.assertIs(calls[0]["load_ui"], False)

    def test_save_is_copy_and_private(self):
        calls = []
        def save(**kwargs):
            calls.append(kwargs)
            Path(kwargs["filepath"]).write_bytes(b"fixture")
            return {"FINISHED"}
        self.bpy.ops.wm = types.SimpleNamespace(save_as_mainfile=save)
        self.commands("blender.file.save", {"path": "safe.blend"})
        self.assertIs(calls[0]["copy"], True)
        self.assertEqual(stat.S_IMODE((Path(self.temp.name) / "safe.blend").stat().st_mode), 0o600)

    def test_render_restores_settings(self):
        render = self.bpy.context.scene.render
        render.filepath = "previous"
        render.image_settings.file_format = "JPEG"
        def fail(**kwargs):
            raise RuntimeError("fixture render failure")
        self.bpy.ops.render = types.SimpleNamespace(render=fail)
        with self.assertRaises(RuntimeError):
            self.commands("blender.render", {"path": "render.png"})
        self.assertEqual(render.filepath, "previous")
        self.assertEqual(render.image_settings.file_format, "JPEG")


if __name__ == "__main__":
    unittest.main()
