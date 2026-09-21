"""Bounded Unix transport. Socket threads never access bpy; the timer drains a queue."""
import json
import os
import queue
import select
import socket
import stat
import struct
import threading
import time
from dataclasses import dataclass, field
from pathlib import Path

from .validation import CommandError

MAX_FRAME = 1_048_576


def _unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("Duplicate JSON key")
        result[key] = value
    return result


def read_frame(stream):
    def exact(size):
        pieces = []
        while size:
            block = stream.recv(size)
            if not block:
                raise EOFError("Truncated frame")
            size -= len(block)
            pieces.append(block)
        return b"".join(pieces)
    length = struct.unpack(">I", exact(4))[0]
    if not 0 < length <= MAX_FRAME:
        raise ValueError("Invalid frame size")
    return json.loads(exact(length), object_pairs_hook=_unique_object,
                      parse_constant=lambda _: (_ for _ in ()).throw(ValueError("Non-finite number")))


def write_frame(stream, value):
    body = json.dumps(value, separators=(",", ":"), ensure_ascii=False, allow_nan=False).encode()
    if not 0 < len(body) <= MAX_FRAME:
        raise ValueError("Output exceeds frame limit")
    stream.sendall(struct.pack(">I", len(body)) + body)


def private_directory(path):
    path = Path(path)
    if not path.exists():
        path.mkdir(mode=0o700)
    meta = path.lstat()
    if not stat.S_ISDIR(meta.st_mode) or meta.st_uid != os.getuid() or stat.S_IMODE(meta.st_mode) != 0o700:
        raise PermissionError("Directory must be owned by this user and mode 0700")
    return path


@dataclass
class Job:
    request: dict
    deadline: float
    done: threading.Event = field(default_factory=threading.Event)
    cancelled: threading.Event = field(default_factory=threading.Event)
    response: dict | None = None


class Server:
    def __init__(self, path, dispatch):
        self.path = Path(path)
        self.dispatch = dispatch
        self.jobs = queue.Queue(maxsize=32)
        self.stopped = threading.Event()
        self.slots = threading.BoundedSemaphore(8)
        self.listener = None
        self.thread = None
        self.connections = set()
        self.lock = threading.Lock()
        self.inode = None

    def start(self):
        private_directory(self.path.parent)
        if self.path.exists() or self.path.is_symlink():
            # Never remove another process's live or stale socket implicitly.
            raise FileExistsError("Bridge socket exists; stop its owner or remove it after inspection")
        self.listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        try:
            self.listener.bind(str(self.path))
            os.chmod(self.path, 0o600)
            self.inode = self.path.lstat().st_ino
            self.listener.listen(8)
            self.listener.settimeout(0.25)
        except BaseException:
            self.listener.close()
            self.listener = None
            raise
        self.thread = threading.Thread(target=self._accept, name="semwright-blender-accept", daemon=True)
        self.thread.start()

    def _accept(self):
        while not self.stopped.is_set():
            try:
                connection, _ = self.listener.accept()
            except socket.timeout:
                continue
            except OSError:
                break
            _, uid, _ = struct.unpack("3i", connection.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12))
            if uid != os.getuid() or not self.slots.acquire(blocking=False):
                connection.close()
                continue
            with self.lock:
                self.connections.add(connection)
            threading.Thread(target=self._serve, args=(connection,), daemon=True).start()

    def _serve(self, connection):
        job = None
        try:
            connection.settimeout(5)
            if read_frame(connection) != {"type": "hello", "protocol": 1}:
                return
            write_frame(connection, {"type": "ready", "protocol": 1, "name": "blender"})
            request = read_frame(connection)
            if (not isinstance(request, dict) or set(request) != {"type", "id", "command", "args"}
                    or request["type"] != "execute" or not isinstance(request["id"], str)
                    or len(request["id"]) != 32 or any(c not in "0123456789abcdef" for c in request["id"])
                    or not isinstance(request["command"], str) or not isinstance(request["args"], dict)):
                return
            job = Job(request, time.monotonic() + 120)
            self.jobs.put_nowait(job)
            while not job.done.wait(0.1):
                if self.stopped.is_set() or time.monotonic() >= job.deadline:
                    job.cancelled.set()
                    return
                # A closed broker connection prevents work that has not reached the main thread.
                try:
                    if select.select([connection], [], [], 0)[0] and connection.recv(1, socket.MSG_PEEK | socket.MSG_DONTWAIT) == b"":
                        job.cancelled.set()
                        return
                except (BlockingIOError, socket.timeout):
                    pass
            write_frame(connection, job.response)
        except (OSError, ValueError, EOFError, queue.Full, TypeError):
            if job is not None:
                job.cancelled.set()
        finally:
            with self.lock:
                self.connections.discard(connection)
            connection.close()
            self.slots.release()

    def drain(self, max_jobs=4):
        """Call only from Blender's main-thread timer (or a deterministic test driver)."""
        for _ in range(max_jobs):
            try:
                job = self.jobs.get_nowait()
            except queue.Empty:
                break
            if job.cancelled.is_set() or time.monotonic() >= job.deadline:
                job.done.set()
                continue
            try:
                result = self.dispatch(job.request["command"], job.request["args"])
                job.response = {"id": job.request["id"], "ok": True, "data": result}
            except CommandError as error:
                job.response = {"id": job.request["id"], "ok": False, "error": {"code": error.code}}
            except Exception:
                # No Python tracebacks/paths/document contents are returned or logged.
                job.response = {"id": job.request["id"], "ok": False, "error": {"code": "BackendFailed"}}
            job.done.set()
        return None if self.stopped.is_set() else 0.02

    def stop(self):
        self.stopped.set()
        if self.listener is not None:
            self.listener.close()
        with self.lock:
            for connection in tuple(self.connections):
                try:
                    connection.shutdown(socket.SHUT_RDWR)
                except OSError:
                    pass
        if self.thread is not None:
            self.thread.join(timeout=1)
        try:
            if self.path.lstat().st_ino == self.inode:
                self.path.unlink()
        except FileNotFoundError:
            pass
