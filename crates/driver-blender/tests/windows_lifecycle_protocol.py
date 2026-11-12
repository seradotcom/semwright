"""Bounded Driver Protocol v8 lifecycle fixture; no Blender or native acceptance.

Run the actual Windows driver against a simulated Host to check ordering,
caller isolation and partial outcomes. Physical Save/Reopen is a separate gate.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import queue
import struct
import subprocess
import threading


class Driver:
    def __init__(self, executable):
        self.process = subprocess.Popen(
            [str(Path(executable).resolve())],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
            env=dict(os.environ, SEMWRIGHT_DRIVER_HOST_TOOLS="1"),
        )
        self.frames = queue.Queue(maxsize=16)
        self.counter = 0
        threading.Thread(target=self._reader, daemon=True).start()
        self.send({
            "type": "hello", "protocol": 8,
            "provider": {
                "id": "driver:blender", "kind": "driver", "version": "1.0.0",
                "namespace": "driver.blender.", "application": "org.blender.Blender",
                "origin": "driver-manifest:lifecycle-protocol-fixture",
            },
            "executable_sha256": "a" * 64,
        })
        assert self.receive()["type"] == "ready"
        self.send({"type": "interfaces", "id": "interfaces"})
        assert self.receive()["interfaces"]["host_tools"] is True
        self.send({"type": "capabilities", "id": "catalog"})
        catalog = self.receive()
        assert catalog["type"] == "capabilities"
        self.catalog = {row["descriptor"]["name"]: row["descriptor"] for row in catalog["capabilities"]}

    def _reader(self):
        def exact(size):
            data = bytearray()
            while len(data) < size:
                part = self.process.stdout.read(size - len(data))
                if not part:
                    raise EOFError("driver closed its protocol")
                data.extend(part)
            return bytes(data)
        try:
            while True:
                size = struct.unpack(">I", exact(4))[0]
                if not 0 < size <= 1_048_576:
                    raise ValueError("unbounded driver frame")
                self.frames.put(json.loads(exact(size)))
        except Exception as error:
            self.frames.put(error)

    def send(self, value):
        data = json.dumps(value, separators=(",", ":")).encode()
        assert 0 < len(data) <= 1_048_576
        self.process.stdin.write(struct.pack(">I", len(data)) + data)
        self.process.stdin.flush()

    def receive(self):
        value = self.frames.get(timeout=5)
        if isinstance(value, Exception):
            raise value
        return value

    def execute(self, command, args, caller="caller-a"):
        self.counter += 1
        request = f"execute-{self.counter}"
        digest = hashlib.sha256(json.dumps(
            self.catalog[command], separators=(",", ":"), ensure_ascii=False
        ).encode()).hexdigest()
        self.send({
            "type": "execute", "id": request, "command": command,
            "descriptor_sha256": digest, "args": args, "context": {"session": caller},
        })
        return request

    def complete(self, request, error=None):
        reply = self.receive()
        assert reply.get("id") == request, reply
        if error:
            assert reply["type"] == "failure" and reply["error"]["code"] == error, reply
            return None
        assert reply["type"] == "result", reply
        return reply["value"]

    def native(self, request, session, command, data=None, error=None):
        frame = self.receive()
        assert frame["type"] == "tool_session_request", frame
        assert frame["parent"] == request and frame["session"] == session, frame
        payload = json.loads(bytes(frame["payload"]))
        assert payload["command"] == command, payload
        reply = {"ok": False, "error": {"code": error}} if error else {"ok": True, "data": data}
        self.send({
            "type": "tool_session_frame", "id": frame["id"], "session": session,
            "payload": list(json.dumps(reply, separators=(",", ":")).encode()),
        })
        return payload["args"]

    def open(self, path, digest, sequence):
        request = self.execute("driver.blender.session.open", {"path": path, "expected_sha256": digest})
        start = self.receive()
        assert start["type"] == "tool_session_start_v8" and start["parent"] == request, start
        assert start["name"] == "blender" and start["args"] == [] and start.get("cwd") is None, start
        session = {"id": f"fixture-session-{sequence}"}
        self.send({"type": "tool_session_started", "id": start["id"], "session": session})
        self.native(request, session, "driver.blender.introspect.summary", {
            "version": [5, 2, 1], "version_string": "5.2.1 LTS",
        })
        opened = {
            "opened": True, "path": path, "sha256": digest, "bytes": 128,
            "use_scripts": False, "fresh_session": True,
        }
        args = self.native(request, session, "driver.blender.file.open_initial", opened)
        assert args == {"path": path, "expected_sha256": digest}
        assert self.complete(request) == opened
        return session

    def stop(self):
        try:
            if self.process.poll() is None:
                self.send({"type": "shutdown", "id": "shutdown"})
                assert self.receive()["type"] == "shutdown"
                assert self.process.wait(timeout=5) == 0
        finally:
            if self.process.poll() is None:
                self.process.terminate()
                self.process.wait(timeout=5)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--driver", required=True)
    args = parser.parse_args()
    driver = Driver(args.driver)
    try:
        inspect = "driver.blender.session.checkpoint.inspect"
        close = "driver.blender.session.checkpoint_close"
        request = driver.execute(inspect, {})
        empty = driver.complete(request)
        assert empty["session_open"] is False and empty["reason"] == "no_session"
        request = driver.execute(close, {"checkpoint_path": "new.blend"})
        assert driver.complete(request) == {"closed": False, "close_state": "no_session", "checkpoint": None}
        request = driver.execute(close, {"checkpoint_path": "new.blend", "session": "foreign"})
        driver.complete(request, error="InvalidArgument")

        session = driver.open("input.blend", "a" * 64, 1)
        request = driver.execute("driver.blender.session.open", {"path": "other.blend", "expected_sha256": "b" * 64})
        driver.complete(request, error="Conflict")
        # A different caller cannot inspect or close the current caller's handle.
        request = driver.execute(close, {"checkpoint_path": "foreign.blend"}, caller="caller-b")
        assert driver.complete(request)["close_state"] == "no_session"

        # Failed native checkpoint must not issue a Host close or discard the handle.
        request = driver.execute(close, {"checkpoint_path": "unsupported.blend"})
        driver.native(request, session, "driver.blender.file.checkpoint", error="Unsupported")
        driver.complete(request, error="Unsupported")

        checkpoint = {
            "path": "recovery.blend", "bytes": 128, "sha256": "c" * 64,
            "copy": True, "contract": "static_mesh_scene_v1",
            "undo_history_preserved": False, "source_fake_users_restored": True,
            "empty_runtime_image_placeholders": 0, "retained_orphans": [],
        }
        request = driver.execute(close, {"checkpoint_path": "recovery.blend"})
        driver.native(request, session, "driver.blender.file.checkpoint", dict(checkpoint, path="foreign.blend"))
        driver.complete(request, error="ProtocolMismatch")

        # An unconfirmed close retains both the durable receipt and the handle.
        request = driver.execute(close, {"checkpoint_path": "recovery.blend"})
        driver.native(request, session, "driver.blender.file.checkpoint", checkpoint)
        closing = driver.receive()
        assert closing["type"] == "tool_session_close" and closing["session"] == session
        driver.send({"type": "tool_session_failure", "id": closing["id"], "error": {
            "code": "Unavailable", "message": "simulated close response loss", "outcome_known": False,
        }})
        partial = driver.complete(request)
        assert partial == {"closed": False, "close_state": "unconfirmed", "checkpoint": checkpoint}
        request = driver.execute("driver.blender.session.open", {"path": "recovery.blend", "expected_sha256": "c" * 64})
        driver.complete(request, error="Conflict")

        # Reconcile the uncertain close by observing the original live session.
        request = driver.execute(inspect, {})
        inspection = {
            "supported": True, "contract": "static_mesh_scene_v1",
            "reason": None, "code": None,
            "inventory": {"objects": 1, "meshes": 1, "scenes": 1, "orphan_datablocks": 0,
                          "empty_runtime_images": 0, "palettes": 0,
                          "empty_sequence_editors": 0, "sequence_channels": 0},
        }
        driver.native(request, session, "driver.blender.file.checkpoint_inspect", inspection)
        assert driver.complete(request) == dict(inspection, session_open=True)

        # Only after reconciliation, an explicit new checkpoint can complete.
        request = driver.execute(close, {"checkpoint_path": "another.blend"})
        saved = dict(checkpoint, path="another.blend")
        driver.native(request, session, "driver.blender.file.checkpoint", saved)
        closing = driver.receive()
        assert closing["type"] == "tool_session_close" and closing["session"] == session
        driver.send({"type": "tool_session_closed", "id": closing["id"], "session": session})
        assert driver.complete(request) == {"closed": True, "close_state": "closed", "checkpoint": saved}
        next_session = driver.open("another.blend", "c" * 64, 2)
        assert next_session != session
        print(json.dumps({
            "protocol_fixture": "PASS", "native_blender_accepted": False,
            "close_after_verified_checkpoint_only": True,
            "failed_checkpoint_and_partial_close_retain_handle": True,
            "foreign_caller_and_existing_session_rejected": True,
            "fresh_open_hash_bound": True,
        }))
    finally:
        driver.stop()


if __name__ == "__main__":
    main()
