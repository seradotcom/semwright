#!/usr/bin/env python3
"""Exact-SHA, nonzero-test runner shared by Actions and CircleCI."""
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
    parser.add_argument("suite", choices=["repository", "file-profile", "portable", "driver", "binding", "package-clean-room", "native-host"])
    args = parser.parse_args()
    provider = "github-actions" if os.getenv("GITHUB_ACTIONS") == "true" else "circleci" if os.getenv("CIRCLECI") == "true" else "local"
    sha = git("rev-parse", "HEAD")
    expected = os.getenv("EXPECTED_SHA") or os.getenv("CIRCLE_SHA1")
    if provider == "local" or expected != sha:
        raise SystemExit("A CI provider and exact expected SHA are required")
    out = ROOT / "verification/native-sdk" / provider / args.suite
    out.mkdir(parents=True, exist_ok=True)
    report = {
        "schema_version": 1, "sha": sha, "tree": git("rev-parse", "HEAD^{tree}"),
        "suite": args.suite, "ci_provider": provider, "os": platform.platform(),
        "profile": args.suite, "native_host_acceptance": False,
        "script_sha256": digest(Path(__file__)),
        "lock_sha256": digest(ROOT / "Cargo.lock"),
        "workflow_sha256": digest(ROOT / ".github/workflows/native-sdk.yml"),
        "status": "FAIL", "test_executions": 0, "skipped": 0, "commands": [],
    }
    if args.suite == "repository":
        commands = [[sys.executable, "-m", "unittest", "discover", "-s", "scripts/native-sdk", "-p", "test_repository_contract.py", "-v"]]
    elif args.suite == "native-host":
        commands = [
            [sys.executable, "scripts/native-sdk/host_e2e.py"],
            ["cargo", "test", "--locked", "-p", "semwright-driver-host", "--features", "test-tools", "--test", "runtime_tools_linux", "linux_v6_runtime_tool_jobs_are_detached_session_bound_and_cancellable", "--", "--ignored", "--nocapture"],
            ["cargo", "test", "--locked", "-p", "semwright-driver-host", "--features", "test-tools", "--test", "adversarial_sandbox", "hostile_driver_is_confined_and_descendants_die_with_provider", "--", "--ignored", "--nocapture"],
            ["cargo", "test", "--locked", "-p", "semwright-core", "--test", "provider_runtime", "--", "--nocapture"],
        ]
    elif args.suite == "package-clean-room":
        commands = [[sys.executable, "scripts/native-sdk/package_clean_room.py"]]
    elif args.suite == "binding":
        commands = [
            [sys.executable, "scripts/native-sdk/typescript_suite.py"],
            [sys.executable, "scripts/native-sdk/clean_consumers.py"],
            ["cargo", "test", "--locked", "-p", "semwright-native-sdk", "--features", "process-bridge", "--lib", "process_bridge", "--", "--nocapture"],
            ["cargo", "test", "--locked", "-p", "semwright-driver-sdk", "materialized_lifecycle_tests", "--", "--nocapture"],
        ]
    elif args.suite in {"portable", "driver"}:
        command = ["cargo", "test", "--locked", "-p", "semwright-native-sdk", "--no-default-features"]
        if args.suite == "driver":
            command += ["--features", "driver"]
            commands = [
                command + ["--test", "cooperation", "--", "--nocapture"],
                ["cargo", "test", "--locked", "-p", "semwright-native-sdk", "--no-default-features", "--features", "graph", "--test", "graph_adapter", "--", "--nocapture"],
                ["cargo", "test", "--locked", "-p", "semwright-effect-conformance", "--test", "prepared_consumer", "--", "--nocapture"],
                ["cargo", "test", "--locked", "-p", "semwright-driver-sdk", "--lib", "--", "--nocapture"],
                ["cargo", "test", "--locked", "-p", "semwright-project-graph", "--test", "graph", "--", "--nocapture"],
                ["cargo", "test", "--locked", "-p", "semwright-semantic-composition", "--test", "contracts", "--", "--nocapture"],
            ]
        else:
            commands = [command + ["--test", "cooperation", "--", "--nocapture"]]
    else:
        base = ["cargo", "test", "--locked", "-p", "semwright-native-sdk", "--features", "file-backed,effects,package"]
        commands = [base + selection + ["--", "--nocapture"] for selection in [
            ["--lib"], ["--test", "conformance"], ["--test", "effects_conformance"],
            ["--test", "export_effects_cli"], ["--test", "package_cli"],
            ["--example", "native-counter"], ["--example", "native-table"],
            ["--bin", "semwright-native-package"],
        ]]
    passed = True
    try:
        for index, command in enumerate(commands):
            print("RUN", " ".join(command), flush=True)
            log_path = out / f"{index:02}.log"
            with log_path.open("w") as log:
                process = subprocess.Popen(command, cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
                assert process.stdout is not None
                for line in process.stdout:
                    print(line, end="", flush=True)
                    log.write(line)
                code = process.wait()
            text = log_path.read_text()
            # Count actual executions across unittest-style wrappers and Rust.
            # Node is wrapped by typescript_suite.py, which emits a conventional
            # "Ran N tests" only after TAP reports zero failed/skipped/todo.
            count = sum(map(int, re.findall(r"Ran (\d+) tests?", text)))
            count += sum(map(int, re.findall(r"test result: ok\. (\d+) passed", text)))
            skipped = sum(map(int, re.findall(r"skipped=(\d+)", text)))
            skipped += sum(map(int, re.findall(r"; (\d+) ignored", text)))
            ok = code == 0 and count > 0 and skipped == 0
            report["commands"].append({"argv": command, "exit_code": code, "tests": count, "skipped": skipped, "status": "PASS" if ok else "FAIL"})
            report["test_executions"] += count
            report["skipped"] += skipped
            if not ok:
                passed = False
                break
        report["status"] = "PASS" if passed else "FAIL"
        report["native_host_acceptance"] = bool(passed and args.suite == "native-host")
    finally:
        report["lock_unchanged"] = digest(ROOT / "Cargo.lock") == report["lock_sha256"]
        if not report["lock_unchanged"]:
            passed = False
            report["status"] = "FAIL"
        (out / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    raise SystemExit(0 if passed else 1)

if __name__ == "__main__": main()
