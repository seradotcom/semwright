#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Independent deterministic fake: protobuf bytes over SP IPC v0/REQ-REP v0.

Not KiCad, not libnng. This fixture validates the client's subset, not actual NNG
or live application compatibility. It never reads a KiCad project from disk.
"""
from __future__ import annotations
import argparse
import copy
import json
import os
import socket
import struct
import threading
import time
from pathlib import Path

PREFIX = "type.googleapis.com/"
COMMON = "kiapi.common.commands."
BOARD = "kiapi.board.commands."
KINDS = {1: "FootprintInstance", 2: "Pad", 11: "Track", 12: "Via", 16: "Zone"}
IDS = {name: f"00000000-0000-4000-8000-{i:012d}" for i, name in enumerate(KINDS.values(), 1)}
COMMIT_ID = "99999999-0000-4000-8000-000000000001"
TOKEN = "self-authored-fixture-instance-not-a-real-token"


def vi(value: int) -> bytes:
    value &= (1 << 64) - 1
    out = bytearray()
    while value > 127:
        out.append((value & 127) | 128)
        value >>= 7
    return bytes(out + bytes([value]))


def var(number: int, value: int) -> bytes:
    return vi(number << 3) + vi(value)


def data(number: int, value: bytes | str) -> bytes:
    if isinstance(value, str):
        value = value.encode()
    return vi(number << 3 | 2) + vi(len(value)) + value


def integer(buf: bytes, offset: int) -> tuple[int, int]:
    result = 0
    for shift in range(0, 70, 7):
        if offset >= len(buf):
            raise ValueError("truncated varint")
        b = buf[offset]
        offset += 1
        result |= (b & 127) << shift
        if b < 128:
            return result, offset
    raise ValueError("integer overflow")


def fields(buf: bytes) -> dict[int, list[int | bytes]]:
    out: dict[int, list[int | bytes]] = {}
    pos = 0
    while pos < len(buf):
        tag, pos = integer(buf, pos)
        number, kind = tag >> 3, tag & 7
        if number == 0:
            raise ValueError("invalid tag")
        if kind == 0:
            value, pos = integer(buf, pos)
        elif kind in (1, 2, 5):
            if kind == 2:
                size, pos = integer(buf, pos)
            else:
                size = 8 if kind == 1 else 4
            if size > len(buf) - pos:
                raise ValueError("truncated field")
            value, pos = buf[pos:pos+size], pos+size
        else:
            raise ValueError("unsupported wire kind")
        out.setdefault(number, []).append(value)
    return out


def bfield(buf: bytes, number: int) -> bytes:
    value = fields(buf).get(number, [b""])[0]
    if not isinstance(value, bytes):
        raise ValueError("field is not bytes")
    return value


def any_message(name: str, body: bytes) -> bytes:
    return data(1, PREFIX+name) + data(2, body)


def vector(x: int, y: int) -> bytes:
    return var(1, x) + var(2, y)


def document(filename: str = "disposable.kicad_pcb", path: str = "/fixture") -> bytes:
    return var(1, 3) + data(4, filename) + data(5, data(1, "disposable") + data(2, path))


def default_items() -> dict[str, bytes]:
    net = data(2, "fixture-net")
    out = {}
    out["Track"] = data(1, data(1, IDS["Track"])) + data(2, vector(1000000, 2000000)) + data(3, vector(3000000, 2000000)) + data(4, var(1, 250000)) + var(5, 1) + var(6, 1) + data(7, net) + data(100, b"unknown-future-field-must-survive")
    out["Via"] = data(1, data(1, IDS["Via"])) + data(2, vector(3000000, 2000000)) + data(3, b"opaque-padstack") + var(4, 1) + data(5, net) + var(6, 1)
    label = data(3, data(2, data(5, "R1")))
    out["FootprintInstance"] = data(1, data(1, IDS["FootprintInstance"])) + data(2, vector(5000000, 6000000)) + data(3, vi(9) + struct.pack("<d", 90.0)) + var(4, 1) + var(5, 1) + data(7, label)
    out["Pad"] = data(1, data(1, IDS["Pad"])) + var(2, 1) + data(3, "1") + data(4, net) + data(7, vector(500000, 0))
    out["Zone"] = data(1, data(1, IDS["Zone"])) + data(5, "fixture-zone")
    return out


def recv_exact(conn: socket.socket, count: int) -> bytes:
    out = bytearray()
    while len(out) < count:
        part = conn.recv(count-len(out))
        if not part:
            raise EOFError
        out += part
    return bytes(out)


class FakeKiCad:
    def __init__(self, path: Path, version: str = "10.0.6", token: str = TOKEN):
        self.path, self.version, self.token = path, version, token
        self.documents = [document()]
        self.items = default_items()
        self.selection: set[str] = set()
        self.nets = ["fixture-net"]
        self.behavior: dict[str, str] = {}
        self.calls: list[str] = []  # Never stores token/request bodies.
        self.active_connections = 0
        self.max_connections = 0
        self.connection_count = 0
        self.commit: dict[str, dict[str, bytes]] = {}
        self.staged: dict[str, dict[str, bytes]] = {}
        self.update_count = 0
        self.stop_event = threading.Event()
        self.lock = threading.RLock()
        self.connections: list[socket.socket] = []
        self.threads: list[threading.Thread] = []

    def __enter__(self):
        self.path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        self.listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.listener.bind(str(self.path))
        os.chmod(self.path, 0o600)
        self.listener.listen(4)
        self.listener.settimeout(0.1)
        self.thread = threading.Thread(target=self.accept, daemon=True)
        self.thread.start()
        return self

    def accept(self):
        while not self.stop_event.is_set():
            try:
                conn, _ = self.listener.accept()
            except socket.timeout:
                continue
            except OSError:
                return
            with self.lock:
                self.connections.append(conn)
                self.connection_count += 1
            t = threading.Thread(target=self.serve, args=(conn,), daemon=True)
            self.threads.append(t)
            t.start()

    def __exit__(self, *_):
        self.stop_event.set()
        self.listener.close()
        for conn in self.connections:
            try:
                conn.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass
            conn.close()
        self.thread.join(2)
        for t in self.threads:
            t.join(2)
        self.path.unlink(missing_ok=True)

    def envelope(self, name: str, payload: bytes, status: int = 1) -> bytes:
        return data(1, data(1, self.token)) + data(2, var(1, status) + (data(2, "untrusted backend diagnostic omitted by client") if status != 1 else b"")) + data(3, any_message(name, payload))

    def serve(self, conn: socket.socket):
        with self.lock:
            self.active_connections += 1
            self.max_connections = max(self.max_connections, self.active_connections)
        try:
            conn.settimeout(10)
            if recv_exact(conn, 8) != b"\0SP\0\0\x30\0\0":
                return
            conn.sendall(b"\0SP\0\0\x31\0\0")
            while not self.stop_event.is_set():
                header = recv_exact(conn, 9)
                n = struct.unpack(">Q", header[1:])[0]
                if header[0] != 1 or n < 4 or n > 1048580:
                    return
                request = recv_exact(conn, n)
                req_id, envelope = request[:4], request[4:]
                h = bfield(envelope, 1)
                token, client = bfield(h, 1).decode(), bfield(h, 2).decode()
                a = bfield(envelope, 2)
                name = bfield(a, 1).decode().removeprefix(PREFIX)
                body = bfield(a, 2)
                op = name.rsplit(".", 1)[1]
                with self.lock:
                    self.calls.append(op)
                    behavior = self.behavior.get(op, "")
                if token and token != self.token:
                    response = self.envelope(name+"Response", b"", 6)
                elif behavior == "busy":
                    response = self.envelope(name+"Response", b"", 7)
                elif behavior == "timeout_status":
                    response = self.envelope(name+"Response", b"", 2)
                elif behavior in ("hang", "drop"):
                    if behavior == "hang":
                        self.stop_event.wait(0.5)
                    return
                elif behavior == "oversized":
                    conn.sendall(b"\x01"+struct.pack(">Q", 1048581))
                    return
                elif behavior == "malformed":
                    response = b"\xff\xff\xff"
                else:
                    with self.lock:
                        response_name, payload = self.handle(op, body, client)
                    response = self.envelope(response_name, payload)
                    if behavior == "apply_then_drop":
                        return
                if behavior == "wrong_id":
                    req_id = b"\x80\0\0\0"
                conn.sendall(b"\x01"+struct.pack(">Q", len(response)+4)+req_id+response)
        except (EOFError, OSError, ValueError, UnicodeError, KeyError):
            pass
        finally:
            conn.close()
            with self.lock:
                self.active_connections -= 1

    def handle(self, op: str, body: bytes, client: str) -> tuple[str, bytes]:
        if op == "GetVersion":
            numeric = self.version.split("-")[0].split("+")[0]
            major, minor, patch = map(int, numeric.split("."))
            return COMMON+"GetVersionResponse", data(1, var(1, major)+var(2, minor)+var(3, patch)+data(4, self.version))
        if op == "GetOpenDocuments":
            return COMMON+"GetOpenDocumentsResponse", b"".join(data(1, d) for d in self.documents)
        if op == "GetItems":
            header = bfield(body, 1)
            container = bfield(header, 2)
            packed = bfield(body, 2)
            kind, _ = integer(packed, 0)
            name = KINDS[kind]
            result = self.items.get(name)
            if name == "Pad" and bfield(container, 1).decode() != IDS["FootprintInstance"]:
                result = None
            payload = data(1, header)+var(2, 1)
            if result is not None:
                payload += data(3, any_message("kiapi.board.types."+name, result))
            return COMMON+"GetItemsResponse", payload
        if op == "GetNets":
            return BOARD+"NetsResponse", b"".join(data(1, data(2, name)) for name in self.nets)
        if op == "GetBoardEnabledLayers":
            return BOARD+"BoardEnabledLayersResponse", var(1, 2)+data(2, vi(1)+vi(32))
        if op in ("GetSelection", "AddToSelection"):
            if op == "AddToSelection":
                for v in fields(body).get(2, []):
                    self.selection.add(bfield(v, 1).decode())
            selected = [any_message("kiapi.board.types."+name, raw) for name, raw in self.items.items() if IDS[name] in self.selection and name != "Pad"]
            return COMMON+"SelectionResponse", b"".join(data(1, item) for item in selected)
        if op == "ClearSelection":
            self.selection.clear()
            return "google.protobuf.Empty", b""
        if op == "BeginCommit":
            self.commit[client] = copy.deepcopy(self.items)
            self.staged[client] = copy.deepcopy(self.items)
            return COMMON+"BeginCommitResponse", data(1, data(1, COMMIT_ID))
        if op == "UpdateItems":
            header, item = bfield(body, 1), bfield(body, 2)
            name = bfield(item, 1).decode().rsplit(".", 1)[1]
            raw = bfield(item, 2)
            code = 7 if self.behavior.get(op) == "reject_item" else 1
            if code == 1:
                self.staged[client][name] = raw
                self.update_count += 1
            return COMMON+"UpdateItemsResponse", data(1, header)+var(2, 1)+data(3, data(1, var(1, code))+data(2, item))
        if op == "EndCommit":
            action = fields(body).get(2, [0])[0]
            if action == 1:
                self.items = self.staged.pop(client)
                self.commit.pop(client)
            elif action == 2:
                self.items = self.commit.pop(client)
                self.staged.pop(client)
            else:
                raise ValueError("unknown commit action")
            return COMMON+"EndCommitResponse", b""
        raise ValueError("unimplemented fake operation")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--socket", type=Path, required=True)
    parser.add_argument("--version", default="10.0.6")
    args = parser.parse_args()
    with FakeKiCad(args.socket, args.version):
        print(json.dumps({"fixture": True, "ready": True}), flush=True)
        try:
            while True:
                time.sleep(1)
        except KeyboardInterrupt:
            pass


if __name__ == "__main__":
    main()
