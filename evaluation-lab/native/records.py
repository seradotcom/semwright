"""Controller-owned command provenance for mechanical hosted smoke.

Hash chains detect accidental omission/editing; they are not signatures or a
same-UID security boundary. A real model runner must use a separate actor UID.
No origin/native attestation is accepted from a model's own JSON.
"""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import time


def file_digest(path):
    h = hashlib.sha256()
    with Path(path).open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024*1024), b""):
            h.update(chunk)
    return h.hexdigest()


def inventory(root):
    root = Path(root)
    result = {}
    for path in sorted(root.rglob("*")):
        rel = path.relative_to(root)
        if ".godot" in rel.parts or path.suffix == ".blend1":
            continue
        if path.is_symlink():
            raise ValueError("Symlink in attempt outputs")
        if path.is_file():
            result[str(rel)] = file_digest(path)
    return result


class Recorder:
    def __init__(self, directory, identity):
        self.directory = Path(directory)
        self.directory.mkdir(parents=True, exist_ok=False)
        self.identity = identity
        self.events = []
        self.previous = "0"*64

    def run(self, command, *, cwd, label, accepted_codes=(0,), timeout=90):
        if os.environ.get("GITHUB_ACTIONS") != "true" or os.environ.get("RUNNER_ENVIRONMENT") != "github-hosted":
            raise RuntimeError("Native commands run only on GitHub-hosted Actions")
        start = time.monotonic_ns()
        result = subprocess.run(command, cwd=cwd, capture_output=True, timeout=timeout)
        elapsed = (time.monotonic_ns()-start)//1_000_000
        log = self.directory / ("%03d-%s.log" % (len(self.events), label))
        log.write_bytes(result.stdout+b"\nSTDERR\n"+result.stderr)
        text = log.read_text(errors="replace")
        event = {"sequence": len(self.events), "previous_sha256": self.previous,
                 "identity": self.identity, "command": [str(c) for c in command],
                 "cwd": str(cwd), "label": label, "returncode": result.returncode,
                 "runtime_ms": elapsed, "log_sha256": file_digest(log),
                 "controller_uid": os.getuid(), "execution_kind": "NATIVE_HARNESS_SMOKE"}
        self.previous = hashlib.sha256(json.dumps(event, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
        event["event_sha256"] = self.previous
        self.events.append(event)
        (self.directory / "commands.json").write_text(json.dumps(self.events, indent=2)+"\n")
        if result.returncode not in accepted_codes:
            raise RuntimeError(label+" failed; see "+str(log))
        if result.returncode == 0 and ("SCRIPT ERROR:" in text or "Traceback (most recent call last)" in text):
            raise RuntimeError(label+" emitted a native script error despite exit zero")
        return result


def verify_product(product, freeze):
    product = Path(product)
    source = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=product).decode().strip()
    if source != freeze["SEMWRIGHT_EVAL_SHA"]:
        raise ValueError("Product checkout differs from I freeze")
    subprocess.run(["git", "diff", "--exit-code"], cwd=product, check=True)
    subprocess.run(["git", "diff", "--cached", "--exit-code"], cwd=product, check=True)
    for name, expected in freeze["source_files_sha256"].items():
        if file_digest(product / name) != expected:
            raise ValueError("Frozen product file differs: "+name)
    return {"source_sha": source, "verified_files": len(freeze["source_files_sha256"]),
            "product_modified": False, "product_commands_executed": False}


def verify_chain(events, expected_identity):
    if not events:
        raise ValueError("Empty controller command record")
    previous = "0"*64
    for sequence, event in enumerate(events):
        payload = dict(event)
        claimed = payload.pop("event_sha256")
        if (payload["sequence"] != sequence or payload["previous_sha256"] != previous
                or payload["identity"] != expected_identity):
            raise ValueError("Controller record identity/order mismatch")
        actual = hashlib.sha256(json.dumps(payload, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
        if actual != claimed:
            raise ValueError("Controller record hash mismatch")
        previous = actual
    return previous
