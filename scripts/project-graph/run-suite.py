#!/usr/bin/env python3
"""Allowlisted Project Graph lane. Heavy work is restricted to GitHub Actions."""
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time

if os.environ.get("GITHUB_ACTIONS") != "true":
    raise SystemExit("Project Graph suites require GitHub Actions")
if len(sys.argv) != 2 or sys.argv[1] != "contracts":
    raise SystemExit("unknown Project Graph suite")
root = Path(__file__).resolve().parents[2]
os.chdir(root)
out = root / "verification/project-graph"
out.mkdir(parents=True, exist_ok=True)
sha = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
expected = os.environ.get("EXPECTED_SHA") or os.environ["GITHUB_SHA"]
if sha != expected:
    raise SystemExit("checkout does not match expected source SHA")
start = time.monotonic()
report = {"schema_version": 1, "role": "C", "source_sha": sha, "workflow_sha": os.environ["GITHUB_SHA"], "contract_sha": "26602e4b25929be869d69ef28fef4dd9713180d7", "workflow": os.environ.get("GITHUB_WORKFLOW"), "run_id": os.environ.get("GITHUB_RUN_ID"), "attempt": os.environ.get("GITHUB_RUN_ATTEMPT"), "job": os.environ.get("GITHUB_JOB"), "job_id": None, "job_id_reason": "collected from Actions API after run", "event": os.environ.get("GITHUB_EVENT_NAME"), "suite": sys.argv[1], "scope": "portable-contract-not-native", "native": False, "outcome": "UNKNOWN", "executed_tests": 0, "ignored_tests": 0, "lock_sha256": hashlib.sha256(Path("Cargo.lock").read_bytes()).hexdigest(), "runtime_versions": {}, "artifacts": {}}
try:
    report["runtime_versions"]["rustc"] = subprocess.check_output(["rustc", "--version"], text=True).strip()
    listing = subprocess.run(["cargo", "test", "--locked", "-p", "semwright-project-graph", "--all-targets", "--", "--list"], text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    print(listing.stdout, flush=True)
    (out / "inventory.log").write_text(listing.stdout)
    if listing.returncode:
        raise RuntimeError("test inventory build failed")
    expected_tests = len(re.findall(r"^.+: test$", listing.stdout, re.MULTILINE))
    report["requested_tests"] = expected_tests
    if expected_tests < 13:
        raise RuntimeError("missing P0 test inventory")
    command = ["cargo", "test", "--locked", "-p", "semwright-project-graph", "--all-targets"]
    report["command"] = command
    result = subprocess.run(command, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    print(result.stdout, flush=True)
    (out / "tests.log").write_text(result.stdout)
    summaries = re.findall(r"^test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out;", result.stdout, re.MULTILINE)
    passed = sum(int(x[0]) for x in summaries)
    failed = sum(int(x[1]) for x in summaries)
    ignored = sum(int(x[2]) for x in summaries)
    report.update(executed_tests=passed + failed, ignored_tests=ignored, failed_tests=failed)
    if result.returncode or failed or ignored or passed != expected_tests:
        raise RuntimeError("executed test inventory does not match requested passing inventory")
    report["outcome"] = "PASS"
except Exception as error:
    report.update(outcome="FAIL", error=str(error))
    raise
finally:
    report["duration_seconds"] = round(time.monotonic() - start, 3)
    (out / "evidence.json").write_text(json.dumps(report, indent=2) + "\n")
