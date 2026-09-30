#!/usr/bin/env python3
"""Direct bounded probe for the LibreOffice protocol-v8 runtime runner."""

from __future__ import annotations

import argparse
import json
import os
import select
import struct
import subprocess
import time
from pathlib import Path

MAX_FRAME = 256 * 1024


def read_exact(stream, size: int, timeout: float) -> bytes:
    data = bytearray()
    deadline = time.monotonic() + timeout
    fd = stream.fileno()
    while len(data) < size:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise TimeoutError("runtime runner response timed out")
        ready, _, _ = select.select([fd], [], [], remaining)
        if not ready:
            raise TimeoutError("runtime runner response timed out")
        chunk = os.read(fd, size - len(data))
        if not chunk:
            raise EOFError("runtime runner closed before one complete frame")
        data.extend(chunk)
    return bytes(data)


def stop(proc: subprocess.Popen[bytes]) -> None:
    if proc.poll() is not None:
        return
    try:
        proc.terminate()
        proc.wait(timeout=3)
    except Exception:
        proc.kill()
        proc.wait(timeout=3)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--runner", required=True)
    parser.add_argument("--workspace", required=True)
    parser.add_argument("--evidence", required=True)
    parser.add_argument("--runtime", default="/usr/lib/libreoffice")
    parser.add_argument("--soffice", default="/usr/lib/libreoffice/program/soffice.bin")
    parser.add_argument("--python", default="/usr/bin/python3")
    args = parser.parse_args()

    runner = Path(args.runner).resolve(strict=True)
    runtime = Path(args.runtime).resolve(strict=True)
    soffice = Path(args.soffice).resolve(strict=True)
    python = Path(args.python).resolve(strict=True)
    workspace = Path(args.workspace)
    evidence = Path(args.evidence)
    evidence.mkdir(parents=True, exist_ok=True)
    workspace.mkdir(parents=True, exist_ok=True)
    workspace.chmod(0o700)

    command = [
        str(runner),
        "--workspace",
        str(workspace.resolve(strict=True)),
        "--runtime",
        str(runtime),
        "--soffice-sealed",
        str(soffice),
        "--python",
        str(python),
    ]
    proc = subprocess.Popen(
        command,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    assert proc.stdin is not None
    assert proc.stdout is not None
    assert proc.stderr is not None

    try:
        payload = json.dumps(
            {"operation": "status", "args": {}},
            separators=(",", ":"),
        ).encode()
        proc.stdin.write(struct.pack(">I", len(payload)) + payload)
        proc.stdin.flush()

        length = struct.unpack(">I", read_exact(proc.stdout, 4, 30.0))[0]
        if not 0 < length <= MAX_FRAME:
            raise RuntimeError(f"runtime runner frame length out of bounds: {length}")
        body = read_exact(proc.stdout, length, 30.0)
        response = json.loads(body)
        evidence.joinpath("direct-runner-status.json").write_text(
            json.dumps(response, indent=2, sort_keys=True) + "\n"
        )
        if response.get("ok") is not True:
            raise RuntimeError(f"runtime runner status failed: {response}")

        proc.stdin.close()
        rc = proc.wait(timeout=10)
        stderr = proc.stderr.read().decode("utf-8", "replace")[-16000:]
        evidence.joinpath("direct-runner.stderr").write_text(stderr)
        if rc != 0:
            raise RuntimeError(f"runtime runner exited with {rc}")
    except Exception:
        stop(proc)
        stderr = proc.stderr.read().decode("utf-8", "replace")[-16000:]
        evidence.joinpath("direct-runner.stderr").write_text(stderr)
        raise

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
