"""Allowlisted CircleCI diagnostics; this file never grants certification."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import time


def suite(name, packages, targets, minimum, features=()):
    command = ["cargo", "test", "--locked"]
    for package in packages:
        command += ["-p", package]
    return name, command + list(features) + list(targets), minimum


LANES = {
    "composition": [
        suite("composition-contracts", ["semwright-semantic-composition", "semwright-media-time"], ["--all-targets"], 65),
        suite("motion-contracts", ["semwright-motion-authoring"], ["--all-targets"], 29),
        suite("av-contracts", ["semwright-av-composition"], ["--lib", "--test", "contracts"], 54),
        suite("audio-model", ["semwright-audio-authoring", "semwright-audio-domain"], ["--lib"], 18),
    ],
    "graph-effects": [
        suite("graph-store", ["semwright-project-graph"], ["--all-targets"], 36, ["--features", "store"]),
        suite("graph-broker", ["semwright-core"], ["--test", "project_graph"], 10),
        suite("effects", ["semwright-effect-conformance"], ["--tests"], 40),
    ],
    "authoring": [
        suite("blender-model", ["semwright-driver-blender"], ["--test", "authoring_model"], 52),
        suite("godot-model", ["semwright-driver-godot"], ["--test", "authoring", "--test", "authoring_store", "--test", "authoring_profile", "--test", "authoring_native"], 51),
        suite("driver-sdk", ["semwright-driver-sdk"], ["--lib"], 26),
    ],
    "smoke": [],
}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("lane", choices=LANES)
    args = parser.parse_args()
    if os.environ.get("CIRCLECI") != "true" or os.environ.get("GITHUB_ACTIONS"):
        raise SystemExit("CircleCI iteration only; never run on the workstation or impersonate Actions")
    source = os.environ["SW_CANDIDATE_SHA"]
    if not re.fullmatch(r"[0-9a-f]{40}", source):
        raise SystemExit("Candidate must be a full immutable SHA")
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
    if head != source or subprocess.check_output(["git", "diff", "--name-only"], text=True).strip():
        raise SystemExit("Candidate source differs from the declared clean checkout")
    out = Path("verification/circleci-integration") / args.lane
    out.mkdir(parents=True, exist_ok=True)
    report = {
        "schema_version": 1, "source_sha": source, "suite_sha": os.environ["CIRCLE_SHA1"],
        "ci_provider": "circleci", "classification": "PRIVATE_ITERATION_DIAGNOSTIC",
        "certification_eligible": False, "native_application_acceptance": False,
        "scope": "portable-model-contracts" if args.lane != "smoke" else "connection-and-source-identity-only",
        "lane": args.lane, "build_url": os.environ.get("CIRCLE_BUILD_URL"),
        "workflow_id": os.environ.get("CIRCLE_WORKFLOW_ID"), "job_id": os.environ.get("CIRCLE_WORKFLOW_JOB_ID"),
        "lock_sha256": hashlib.sha256(Path("Cargo.lock").read_bytes()).hexdigest(),
        "runner": {"os": "linux", "architecture": os.uname().machine},
        "steps": [], "outcome": "FAIL", "r16_closed": False,
    }
    started = time.monotonic()

    def run(name, command):
        remaining = 1500 - (time.monotonic() - started)
        if remaining <= 0:
            raise RuntimeError("Iteration exceeded its 25 minute command budget")
        log = out / (name + ".log")
        with log.open("w") as stream:
            result = subprocess.run(command, stdout=stream, stderr=subprocess.STDOUT, timeout=remaining, check=False)
        content = log.read_text(errors="replace")
        print(content[-20000:], flush=True)
        report["steps"].append({"name": name, "command": command, "exit_code": result.returncode,
                                "log_sha256": hashlib.sha256(log.read_bytes()).hexdigest()})
        if result.returncode:
            raise RuntimeError(name + " failed")
        return content

    try:
        run("source-hygiene", ["git", "diff", "--check", source + "^", source])
        if args.lane != "smoke":
            report["rustc"] = subprocess.check_output(["rustc", "--version"], text=True).strip()
            packages = sorted({command[index + 1] for _, command, _ in LANES[args.lane]
                               for index, word in enumerate(command) if word == "-p"})
            fmt = ["cargo", "fmt"]
            for package in packages:
                fmt += ["-p", package]
            run("format", fmt + ["--", "--check"])
        for name, command, minimum in LANES[args.lane]:
            listing = run(name + "-inventory", command + ["--", "--list"])
            expected = len(re.findall(r"^.+: test$", listing, re.MULTILINE))
            if expected < minimum:
                raise RuntimeError(name + " has zero or incomplete test inventory")
            content = run(name, command + ["--", "--nocapture"])
            rows = re.findall(r"^test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;", content, re.MULTILINE)
            passed, failed, ignored = (sum(int(row[index]) for row in rows) for index in range(3))
            report["steps"][-1].update(requested_tests=expected, passed=passed, failed=failed, ignored=ignored)
            if failed or ignored or passed != expected:
                raise RuntimeError(name + " did not execute the entire requested passing inventory")
        if subprocess.check_output(["git", "diff", "--name-only"], text=True).strip():
            raise RuntimeError("Iteration changed tracked source or locks")
        report["outcome"] = "PASS_DIAGNOSTIC"
    except (RuntimeError, OSError, subprocess.SubprocessError) as error:
        report["error"] = str(error)
    finally:
        report["elapsed_seconds"] = time.monotonic() - started
        (out / "iteration.json").write_text(json.dumps(report, indent=2) + "\n")
    raise SystemExit(0 if report["outcome"] == "PASS_DIAGNOSTIC" else 1)


if __name__ == "__main__":
    main()
