#!/usr/bin/env python3
"""Remote-only narrow Rust lane. Receipt records tests actually listed/executed."""
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time

SUITES = {"effect-contract": "semwright-effect-conformance"}
if len(sys.argv) != 2 or sys.argv[1] not in SUITES:
    raise SystemExit("unknown suite (allowed: effect-contract)")
if os.environ.get("GITHUB_ACTIONS") != "true":
    raise SystemExit("heavy suites run only in GitHub Actions")
root = Path(__file__).resolve().parents[2]
os.chdir(root)
out = root / "verification/effects"
out.mkdir(parents=True, exist_ok=True)
sha = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
if sha != os.environ.get("EXPECTED_SHA"):
    raise SystemExit("checkout differs from expected source SHA")
started = time.monotonic()
receipt = {"schema_version": 1, "role": "effect-conformance", "source_sha": sha,
           "suite_sha": sha, "event_sha": os.environ.get("GITHUB_SHA"),
           "contract_sha": "26602e4b25929be869d69ef28fef4dd9713180d7",
           "dependency_shas": {"composition_c0":"26602e4b25929be869d69ef28fef4dd9713180d7","project_graph_p0":"6ee52b428310370d3ad438a13964086a63f48367"},
           "runner_os": os.environ.get("RUNNER_OS"), "runner_arch": os.environ.get("RUNNER_ARCH"),
           "suite": sys.argv[1], "workflow": os.environ.get("GITHUB_WORKFLOW"),
           "run_id": os.environ.get("GITHUB_RUN_ID"), "attempt": os.environ.get("GITHUB_RUN_ATTEMPT"),
           "job": os.environ.get("GITHUB_JOB"), "job_database_id": None,
           "event": os.environ.get("GITHUB_EVENT_NAME"),
           "runtime": subprocess.check_output(["rustc", "--version"], text=True).strip(),
           "lock_sha256": hashlib.sha256(Path("Cargo.lock").read_bytes()).hexdigest(),
           "features": [], "requested_tests": [], "executed_tests": [], "skipped": [],
           "evidence_scope": "portable-contract-not-native", "outcome": "ERROR",
           "limitations": ["Job database ID is enriched through the GitHub API after the run."]}
try:
    cmd = ["cargo", "test", "--locked", "-p", SUITES[sys.argv[1]], "--tests"]
    listed = subprocess.run(cmd + ["--", "--list"], text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    (out / "list.log").write_text(listed.stdout)
    print(listed.stdout, flush=True)
    if listed.returncode: raise RuntimeError("test discovery/build failed")
    names = re.findall(r"^(.+): test$", listed.stdout, re.M)
    if not names or len(names) != len(set(names)): raise RuntimeError("zero or ambiguous discovered tests")
    receipt["requested_tests"] = names
    tested = subprocess.run(cmd + ["--", "--nocapture"], text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    (out / "tests.log").write_text(tested.stdout)
    print(tested.stdout, flush=True)
    executed = re.findall(r"^test (.+) \.\.\. ok$", tested.stdout, re.M)
    receipt["executed_tests"] = executed
    receipt["skipped"] = re.findall(r"^test (.+) \.\.\. ignored", tested.stdout, re.M)
    if tested.returncode or set(executed) != set(names) or len(executed) != len(names) or receipt["skipped"]:
        raise RuntimeError("test exit/count/identity mismatch")
    schema = subprocess.check_output(["cargo", "run", "--locked", "-p", SUITES[sys.argv[1]], "--example", "schemas"], text=True)
    (out / "effect-contract.schema.json").write_text(schema)
    receipt["outcome"] = "PASS"
finally:
    receipt["duration_seconds"] = round(time.monotonic() - started, 3)
    (out / "manifest.json").write_text(json.dumps(receipt, indent=2) + "\n")
