#!/usr/bin/env python3
"""Allowlisted GitHub-hosted diagnostic suites; zero tests never satisfy a gate."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys

SUITES = {
    "motion": (["semwright-motion-authoring"], 20),
    "contracts": (["semwright-semantic-composition", "semwright-media-time"], 43),
}

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("suite", choices=sorted(SUITES))
    args = parser.parse_args()
    if os.environ.get("GITHUB_ACTIONS") != "true" or os.environ.get("RUNNER_ENVIRONMENT") != "github-hosted":
        parser.error("Compilation is restricted to GitHub-hosted Actions, not the workstation")
    packages, minimum = SUITES[args.suite]
    cmd = ["cargo", "test", "--locked"]
    for package in packages:
        cmd.extend(["-p", package])
    cmd.extend(["--all-targets", "--", "--nocapture"])
    root = Path("verification/composition")
    root.mkdir(parents=True, exist_ok=True)
    log_path = root / (args.suite + ".log")
    with log_path.open("w") as log:
        result = subprocess.run(cmd, stdout=log, stderr=subprocess.STDOUT, check=False)
    text = log_path.read_text()
    print(text[-120000:])
    summaries = re.findall(r"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored", text)
    passed = sum(int(s[0]) for s in summaries)
    failed = sum(int(s[1]) for s in summaries)
    ignored = sum(int(s[2]) for s in summaries)
    ok = result.returncode == 0 and passed >= minimum and failed == 0 and ignored == 0
    report = {
        "schema_version": 1,
        "suite": args.suite,
        "tested_sha": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip(),
        "github_sha": os.environ.get("GITHUB_SHA"),
        "run_id": os.environ.get("GITHUB_RUN_ID"),
        "run_attempt": os.environ.get("GITHUB_RUN_ATTEMPT"),
        "job": os.environ.get("GITHUB_JOB"),
        "runner": os.environ.get("RUNNER_OS"),
        "rustc": subprocess.check_output(["rustc", "--version"], text=True).strip(),
        "lock_sha256": hashlib.sha256(Path("Cargo.lock").read_bytes()).hexdigest(),
        "exit_code": result.returncode,
        "expected_minimum": minimum,
        "passed": passed, "failed": failed, "ignored": ignored,
        "status": "PASS" if ok else "FAIL",
        "evidence_scope": "portable unit/contract tests, not native application acceptance",
    }
    (root / (args.suite + ".json")).write_text(json.dumps(report, indent=2) + "\n")
    if not ok:
        print("suite failed or ran fewer tests than required", file=sys.stderr)
        return 1
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
