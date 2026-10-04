#!/usr/bin/env python3
"""Bounded, exact-source runner shared by Actions and CircleCI."""
from __future__ import annotations
import argparse
import hashlib
import json
import os
import platform
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]

def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def git(*args):
    return subprocess.check_output(["git", *args], cwd=ROOT, text=True).strip()

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("suite", choices=["audit"])
    args = parser.parse_args()
    provider = "github-actions" if os.getenv("GITHUB_ACTIONS") == "true" else "circleci" if os.getenv("CIRCLECI") == "true" else "local"
    sha = git("rev-parse", "HEAD")
    expected = os.getenv("EXPECTED_SHA") or os.getenv("CIRCLE_SHA1")
    if provider == "local" or expected != sha:
        raise SystemExit("A CI provider and exact EXPECTED_SHA/CIRCLE_SHA1 are required")
    out = ROOT / "verification/native-sdk" / provider / args.suite
    out.mkdir(parents=True, exist_ok=True)
    report = {
        "schema_version": 1, "sha": sha, "tree": git("rev-parse", "HEAD^{tree}"),
        "suite": args.suite, "ci_provider": provider, "os": platform.platform(),
        "profile": "metadata-only", "native_acceptance": False,
        "script_sha256": digest(Path(__file__)),
        "lock_sha256": digest(ROOT / "Cargo.lock"),
        "status": "FAIL", "tests": 0, "skipped": 0,
    }
    command = [sys.executable, "-m", "unittest", "discover", "-s", "scripts/native-sdk", "-p", "test_*.py", "-v"]
    result = subprocess.run(command, cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, timeout=90)
    (out / "tests.log").write_text(result.stdout)
    print(result.stdout, end="")
    counts = re.findall(r"Ran (\d+) tests?", result.stdout)
    report["tests"] = sum(map(int, counts))
    report["skipped"] = sum(map(int, re.findall(r"skipped=(\d+)", result.stdout)))
    passed = result.returncode == 0 and report["tests"] > 0 and report["skipped"] == 0
    report["status"] = "PASS" if passed else "FAIL"
    report["command"] = command
    (out / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    raise SystemExit(0 if passed else 1)

if __name__ == "__main__": main()
