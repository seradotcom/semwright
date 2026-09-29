#!/usr/bin/env python3
"""Exact-SHA allowlisted remote diagnostics. Never runs product tests locally."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import time

SUITES = {
    "blender-model": (["cargo", "test", "--locked", "-p", "semwright-driver-blender", "--test", "authoring_model", "--", "--test-threads=1"], 45),
    "blender-native-authoring": (["cargo", "test", "--locked", "-p", "semwright-driver-blender", "--features", "authoring-native", "--test", "authoring_native", "--", "--test-threads=1", "--nocapture"], 3),
}

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--suite", choices=SUITES, required=True)
    args = parser.parse_args()
    if os.environ.get("GITHUB_ACTIONS") != "true" or os.environ.get("RUNNER_ENVIRONMENT") != "github-hosted":
        raise SystemExit("This diagnostic only executes on authorized GitHub-hosted runners.")
    sha = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
    if sha != os.environ.get("GITHUB_SHA"):
        raise SystemExit("Checkout does not match the new push SHA.")
    if subprocess.check_output(["git", "status", "--porcelain", "--untracked-files=no"], text=True).strip():
        raise SystemExit("Tracked source changes would invalidate exact-SHA evidence.")
    command, minimum = SUITES[args.suite]
    out = Path(os.environ["SEMWRIGHT_AUTHORING_EVIDENCE"])
    out.mkdir(parents=True, exist_ok=True)
    start = time.monotonic()
    log = out / (args.suite + ".log")
    with log.open("w") as stream:
        result = subprocess.run(command, stdout=stream, stderr=subprocess.STDOUT, timeout=1500, check=False)
    text = log.read_text()
    print(text[-16000:])
    totals = re.findall(r"test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out", text)
    passed = sum(int(row[1]) for row in totals)
    failed = sum(int(row[2]) for row in totals)
    ignored = sum(int(row[3]) for row in totals)
    filtered = sum(int(row[5]) for row in totals)
    ok = result.returncode == 0 and passed >= minimum and failed == 0 and ignored == 0 and filtered == 0
    if args.suite == "blender-native-authoring":
        receipt = out / "native-pipeline.json"
        native = json.loads(receipt.read_text()) if receipt.exists() else {}
        ok = ok and native.get("version") == 1 and native.get("native_assertions_completed") is True and native.get("writer_process") != native.get("reader_process")
    report = {"schema_version":1,"role":"E","source_sha":sha,"suite_sha":sha,"github_sha":os.environ.get("GITHUB_SHA"),
        "event":os.environ.get("GITHUB_EVENT_NAME"),"workflow":os.environ.get("GITHUB_WORKFLOW"),"run_id":os.environ.get("GITHUB_RUN_ID"),
        "attempt":os.environ.get("GITHUB_RUN_ATTEMPT"),"job_key":os.environ.get("GITHUB_JOB"),"job_database_id":None,
        "contract_sha":"26602e4b25929be869d69ef28fef4dd9713180d7","glb_dependency_sha":"74671c11dda2133ce6af939896c49cdbb6ba47d5",
        "p0_consumed_sha":"6ee52b428310370d3ad438a13964086a63f48367","e0_consumed_sha":"5ed7d0ff8016d76031ec33846f27a237f196835c","suite":args.suite,"command":command,"passed":passed,"failed":failed,
        "ignored":ignored,"filtered":filtered,"rustc":subprocess.check_output(["rustc","--version"],text=True).strip(),
        "lock_sha256":hashlib.sha256(Path("Cargo.lock").read_bytes()).hexdigest(),"elapsed_seconds":time.monotonic()-start,
        "outcome":"PASS" if ok else "FAIL","native_scope":"Broker/Host Blender only" if args.suite != "blender-model" else "none",
        "log_sha256":hashlib.sha256(log.read_bytes()).hexdigest(),"BLENDER_AUTHORING_READY":False}
    (out / (args.suite + ".json")).write_text(json.dumps(report,indent=2)+"\n")
    raise SystemExit(0 if ok else 1)

if __name__ == "__main__": main()
