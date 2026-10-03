#!/usr/bin/env python3
"""Bounded positive checks on frozen source in a disposable hosted runner."""
from __future__ import annotations
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import time

SOURCE = "6491c0d838fa066938a494524d69ed507aa0dbe8"
PACKAGES = ("semwright-policy", "semwright-protocol", "semwright-semantic-composition",
            "semwright-project-graph", "semwright-effect-conformance")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--suite-sha", required=True)
    args = parser.parse_args()
    if os.environ.get("GITHUB_ACTIONS") != "true":
        raise SystemExit("Requires an actual GitHub Actions runner; do not emulate its environment")
    if not re.fullmatch(r"[0-9a-f]{40}", args.suite_sha):
        raise SystemExit("Full suite SHA required")
    source = args.source.resolve(strict=True)
    def git(*parts: str) -> str:
        return subprocess.check_output(["git", *parts], cwd=source, text=True).strip()
    if git("rev-parse", "HEAD") != SOURCE or git("status", "--porcelain", "--untracked-files=no"):
        raise SystemExit("Frozen source must match the reviewed SHA without tracked changes")
    lock_before = hashlib.sha256((source / "Cargo.lock").read_bytes()).hexdigest()
    record = {"schema_version": 1, "source_sha": SOURCE, "suite_sha": args.suite_sha,
              "r16_closed": False, "classification": "POSITIVE_CONTRACT_AND_FAKE_SMOKE_ONLY", "checks": []}
    commands = [
        ["cargo", "build", "--locked", "-p", "semwright-daemon", "-p", "semwright-cli", "--bins"],
        ["cargo", "test", "--locked", *[x for name in PACKAGES for x in ("-p", name)], "--lib"],
        ["bash", "scripts/dev/fake-smoke.sh"],
    ]
    env = os.environ.copy()
    env["BIN_DIR"] = "target/debug"
    failed = False
    for command in commands:
        started = time.monotonic()
        print("R16_COMMAND " + json.dumps(command), flush=True)
        result = subprocess.run(command, cwd=source, env=env, check=False)
        record["checks"].append({"command": command, "returncode": result.returncode,
                                 "seconds": round(time.monotonic() - started, 3)})
        if result.returncode != 0:
            failed = True
            break
    record["source_unchanged"] = not bool(git("status", "--porcelain", "--untracked-files=no"))
    record["lock_unchanged"] = lock_before == hashlib.sha256((source / "Cargo.lock").read_bytes()).hexdigest()
    record["result"] = "PASS" if not failed and record["source_unchanged"] and record["lock_unchanged"] else "FAIL"
    print("R16_SMOKE_RECORD " + json.dumps(record, sort_keys=True), flush=True)
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a", encoding="utf-8") as stream:
            stream.write("## Frozen-source smoke (not security approval)\n```json\n")
            stream.write(json.dumps(record, indent=2) + "\n```\n")
    return 0 if record["result"] == "PASS" else 1


if __name__ == "__main__":
    raise SystemExit(main())
