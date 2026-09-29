#!/usr/bin/env python3
"""Run one allowlisted audio suite with exact-SHA evidence and zero-test detection."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
SUITES = {
    "portable": ["-p", "semwright-audio-authoring", "-p", "semwright-audio-domain", "-p", "semwright-faust-audio-driver", "-p", "semwright-ardour-audio-driver", "--tests"],
    "faust": ["-p", "semwright-faust-audio-driver", "--test", "live_faust"],
    "faust-host": ["-p", "semwright-faust-audio-driver", "--test", "host_conformance"],
    "analysis-host": ["-p", "semwright-faust-audio-driver", "--test", "analysis_host_conformance"],
}


def run(argv, log):
    with log.open("w", encoding="utf-8") as stream:
        result = subprocess.run(argv, cwd=ROOT, stdout=stream, stderr=subprocess.STDOUT, check=False)
    text = log.read_text(encoding="utf-8", errors="replace")
    print(text[-48000:])
    return result.returncode, text


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("suite", choices=sorted(SUITES))
    args = parser.parse_args()
    out = ROOT / "verification" / "audio"
    out.mkdir(parents=True, exist_ok=True)
    sha = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    extra = ["--ignored"] if args.suite in {"faust", "faust-host", "analysis-host"} else []
    command = ["cargo", "test", "--locked", *SUITES[args.suite]]
    receipt = {"schema_version": 1, "tested_sha": sha, "github_sha": os.getenv("GITHUB_SHA"),
               "run_id": os.getenv("GITHUB_RUN_ID"), "run_attempt": os.getenv("GITHUB_RUN_ATTEMPT"),
               "job": os.getenv("GITHUB_JOB"), "suite": args.suite, "status": "FAIL",
               "lock_sha256": hashlib.sha256((ROOT / "Cargo.lock").read_bytes()).hexdigest(),
               "expected_tests": 0, "passed": 0, "failed": 0, "ignored": 0}
    try:
        code, listing = run(command + ["--", "--list", *extra], out / (args.suite + "-list.log"))
        expected = sum(bool(re.match(r"^.+: test$", line)) for line in listing.splitlines())
        receipt["expected_tests"] = expected
        if code or not expected:
            raise RuntimeError("Suite failed to enumerate nonzero tests")
        code, text = run(command + ["--", *extra, "--nocapture"], out / (args.suite + ".log"))
        summaries = re.findall(r"test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;", text)
        for key, index in [("passed", 0), ("failed", 1), ("ignored", 2)]:
            receipt[key] = sum(int(row[index]) for row in summaries)
        if code or receipt["failed"] or receipt["passed"] == 0:
            raise RuntimeError("Suite did not execute successfully")
        if receipt["passed"] + receipt["ignored"] != expected:
            raise RuntimeError("Executed/ignored test count differs from enumeration")
        if args.suite != "portable" and receipt["ignored"]:
            raise RuntimeError("Native suite cannot satisfy acceptance with ignored tests")
        receipt["status"] = "PASS"
    except (RuntimeError, OSError) as error:
        receipt["error"] = str(error)
    (out / (args.suite + ".json")).write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps(receipt, indent=2))
    return 0 if receipt["status"] == "PASS" else 1


if __name__ == "__main__":
    sys.exit(main())
