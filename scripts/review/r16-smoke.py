#!/usr/bin/env python3
"""Bounded positive checks on an exact source SHA in a disposable hosted runner."""
from __future__ import annotations
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import time

TEST_PACKAGES = (
    "semwright-policy",
    "semwright-protocol",
    "semwright-semantic-composition",
)


def emit(result: subprocess.CompletedProcess[str]) -> None:
    if result.stdout:
        print(result.stdout, end="" if result.stdout.endswith("\n") else "\n", flush=True)
    if result.stderr:
        print(result.stderr, end="" if result.stderr.endswith("\n") else "\n", flush=True)


def run(command: list[str], source: Path, env: dict[str, str], capture: bool = False) -> tuple[subprocess.CompletedProcess[str], float]:
    started = time.monotonic()
    print("R16_COMMAND " + json.dumps(command), flush=True)
    result = subprocess.run(
        command,
        cwd=source,
        env=env,
        check=False,
        text=True,
        capture_output=capture,
    )
    if capture:
        emit(result)
    return result, round(time.monotonic() - started, 3)


def listed_test_count(package: str, source: Path, env: dict[str, str]) -> tuple[int, dict]:
    command = ["cargo", "test", "--locked", "-p", package, "--lib", "--", "--list"]
    result, seconds = run(command, source, env, capture=True)
    tests = [line for line in result.stdout.splitlines() if line.rstrip().endswith(": test")]
    return len(tests), {
        "kind": "test_inventory",
        "package": package,
        "command": command,
        "returncode": result.returncode,
        "seconds": seconds,
        "test_count": len(tests),
    }


def assert_fake_smoke(stdout: str) -> list[str]:
    rows = []
    for line in stdout.splitlines():
        line = line.strip()
        if not line.startswith("{"):
            continue
        try:
            value = json.loads(line)
        except json.JSONDecodeError:
            continue
        if isinstance(value, dict) and isinstance(value.get("command"), str):
            rows.append(value)
    by_command = {row["command"]: row for row in rows}
    required = {"doctor", "ui.find", "recipe.run", "audit.tail"}
    missing = sorted(required - set(by_command))
    if missing:
        raise ValueError("fake smoke omitted structured commands: " + ", ".join(missing))
    doctor = by_command["doctor"]
    if doctor.get("ok") is not True or doctor.get("data", {}).get("fake") is not True:
        raise ValueError("doctor did not prove the explicit fake backend")
    discovery = by_command["ui.find"]
    if discovery.get("ok") is not True or discovery.get("data", {}).get("count") != 2:
        raise ValueError("fixture ambiguity was not observed as two candidates")
    recipe = by_command["recipe.run"]
    steps = recipe.get("data", {}).get("steps")
    if (
        recipe.get("ok") is not True
        or recipe.get("data", {}).get("completed") is not True
        or recipe.get("data", {}).get("outputs", {}).get("changed") is not True
        or not isinstance(steps, list)
        or len(steps) != 2
        or not all(step.get("ok") is True for step in steps)
    ):
        raise ValueError("fake recipe did not complete the asserted mutation path")
    events = by_command["audit.tail"].get("data", {}).get("events")
    if not isinstance(events, list) or not any(
        event.get("command") == "ui.invoke"
        and event.get("phase") == "finish"
        and event.get("ok") is True
        for event in events
    ):
        raise ValueError("audit did not retain a successful ui.invoke finish record")
    return [
        "doctor_explicit_fake",
        "ambiguous_discovery_two_candidates",
        "recipe_completed_changed_true",
        "two_recipe_steps_ok",
        "audit_ui_invoke_finish_ok",
    ]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--expected-source-sha", required=True)
    parser.add_argument("--suite-sha", required=True)
    args = parser.parse_args()
    if os.environ.get("GITHUB_ACTIONS") != "true":
        raise SystemExit("Requires an actual GitHub Actions runner; do not emulate its environment")
    for value, label in ((args.expected_source_sha, "source"), (args.suite_sha, "suite")):
        if not re.fullmatch(r"[0-9a-f]{40}", value):
            raise SystemExit(f"Full {label} SHA required")

    source = args.source.resolve(strict=True)

    def git(*parts: str) -> str:
        return subprocess.check_output(["git", *parts], cwd=source, text=True).strip()

    if git("rev-parse", "HEAD") != args.expected_source_sha:
        raise SystemExit("Frozen source checkout does not match the requested source SHA")
    if git("status", "--porcelain", "--untracked-files=no"):
        raise SystemExit("Frozen source has tracked changes")

    lock_before = hashlib.sha256((source / "Cargo.lock").read_bytes()).hexdigest()
    record = {
        "schema_version": 2,
        "source_sha": args.expected_source_sha,
        "suite_sha": args.suite_sha,
        "r16_closed": False,
        "classification": "POSITIVE_SELECTED_CONTRACT_AND_FAKE_SMOKE_ONLY",
        "checks": [],
    }
    env = os.environ.copy()
    env["BIN_DIR"] = "target/debug"
    failed = False

    build = ["cargo", "build", "--locked", "-p", "semwright-daemon", "-p", "semwright-cli", "--bins"]
    result, seconds = run(build, source, env)
    record["checks"].append({
        "kind": "build",
        "command": build,
        "returncode": result.returncode,
        "seconds": seconds,
    })
    failed = result.returncode != 0

    if not failed:
        for package in TEST_PACKAGES:
            count, inventory = listed_test_count(package, source, env)
            record["checks"].append(inventory)
            if inventory["returncode"] != 0 or count == 0:
                failed = True
                break
            command = ["cargo", "test", "--locked", "-p", package, "--lib"]
            result, seconds = run(command, source, env)
            record["checks"].append({
                "kind": "unit_contract",
                "package": package,
                "command": command,
                "returncode": result.returncode,
                "seconds": seconds,
                "expected_test_count": count,
            })
            if result.returncode != 0:
                failed = True
                break

    if not failed:
        command = ["bash", "scripts/dev/fake-smoke.sh"]
        result, seconds = run(command, source, env, capture=True)
        assertions: list[str] = []
        assertion_error = None
        if result.returncode == 0:
            try:
                assertions = assert_fake_smoke(result.stdout)
            except ValueError as error:
                assertion_error = str(error)
        record["checks"].append({
            "kind": "functional_fake_smoke",
            "command": command,
            "returncode": result.returncode,
            "seconds": seconds,
            "assertions": assertions,
            "assertion_error": assertion_error,
        })
        if result.returncode != 0 or assertion_error is not None:
            failed = True

    record["source_unchanged"] = not bool(git("status", "--porcelain", "--untracked-files=no"))
    record["lock_unchanged"] = (
        lock_before == hashlib.sha256((source / "Cargo.lock").read_bytes()).hexdigest()
    )
    record["result"] = (
        "PASS"
        if not failed and record["source_unchanged"] and record["lock_unchanged"]
        else "FAIL"
    )
    print("R16_SMOKE_RECORD " + json.dumps(record, sort_keys=True), flush=True)
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a", encoding="utf-8") as stream:
            stream.write("## Frozen-source smoke (not security approval)\n```json\n")
            stream.write(json.dumps(record, indent=2) + "\n```\n")
    return 0 if record["result"] == "PASS" else 1


if __name__ == "__main__":
    raise SystemExit(main())
