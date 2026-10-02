"""Stable oracle identity across target-only Git commits; not an authorization token."""
from __future__ import annotations
import hashlib
import json
from pathlib import Path
import re
import subprocess

PREFIX = "tests/semantic-adversarial-lab/"
WORKFLOW = ".github/workflows/semantic-adversarial-lab.yml"
TARGET_FIELDS = frozenset({"targets", "selected_lanes", "combined_candidate_sha"})

def payload_digest(files: dict[str, bytes], lock: dict) -> str:
    """Bind executable fixtures, expectations, mutants, runtime/limit/contract pins.

Only explicit target selection, selected lanes and the joint candidate pointer
may change without changing this identity. Markdown/report metadata is not an
oracle. The full source and suite commits are still recorded for every run.
"""
    projection = {key: value for key, value in lock.items() if key not in TARGET_FIELDS}
    payload = {"version": 1, "configuration": projection,
               "files": {name: hashlib.sha256(data).hexdigest() for name, data in sorted(files.items())}}
    return hashlib.sha256(json.dumps(payload, sort_keys=True, separators=(",", ":"), allow_nan=False).encode()).hexdigest()

def from_git(root: Path, suite_sha: str) -> str:
    if not isinstance(suite_sha, str) or not re.fullmatch(r"[0-9a-f]{40}", suite_sha):
        raise ValueError("Full oracle suite SHA required")
    def git(*args: str) -> bytes:
        return subprocess.check_output(["git", "-C", str(root), *args])
    files = {}
    lock = None
    for record in git("ls-tree", "-rz", suite_sha, "--", PREFIX, WORKFLOW).split(b"\0"):
        if not record:
            continue
        header, raw_name = record.split(b"\t", 1)
        mode, kind, _ = header.decode().split()
        name = raw_name.decode("utf-8", errors="strict")
        if mode not in {"100644", "100755"} or kind != "blob":
            raise ValueError("Nonregular oracle source")
        if name == PREFIX + "targets.json":
            lock = json.loads(git("show", suite_sha + ":" + name))
            continue
        if name.endswith(".md") or name == PREFIX + "COVERAGE.json" or name.startswith(PREFIX + "reports/"):
            continue
        data = git("show", suite_sha + ":" + name)
        if len(data) > 1024 * 1024:
            raise ValueError("Oracle file budget")
        files[name] = data
    if lock is None or not files:
        raise ValueError("Missing immutable oracle source/configuration")
    return payload_digest(files, lock)
