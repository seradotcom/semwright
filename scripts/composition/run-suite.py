#!/usr/bin/env python3
"""Allowlisted Composition suites with explicit evidence authority.

GitHub-hosted and dedicated CircleCI candidate modes are certification-eligible.
Ordinary CircleCI iteration remains private diagnostics only. Zero tests never
satisfy any mode.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys

SUITES = {
    # AV native integration is a separate exact-SHA gate. The portable suite
    # must not execute or count its intentionally ignored runtime E2E.
    "av": {
        "packages": ["semwright-av-composition"],
        "targets": ["--lib", "--test", "contracts"],
        "minimum": 50,
    },
    "motion": {
        "packages": ["semwright-motion-authoring"],
        "targets": ["--all-targets"],
        "minimum": 29,
    },
    "contracts": {
        "packages": ["semwright-semantic-composition", "semwright-media-time"],
        "targets": ["--all-targets"],
        "minimum": 65,
    },
}

def suite_minimum(name: str, root: Path = Path(".")) -> int:
    """Return the expected portable inventory for the checked-out product topology."""
    minimum = SUITES[name]["minimum"]
    if (
        name == "av"
        and (root / "crates/audio-authoring").is_dir()
        and (root / "crates/audio-domain").is_dir()
    ):
        # The certified audio integration adds two AV lib tests and two public
        # audio-receipt contract tests. Standalone Agent A must not require B,
        # while a combined workspace must not silently lose those four tests.
        minimum = max(minimum, 54)
    return minimum


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("suite", choices=sorted(SUITES))
    parser.add_argument(
        "--evidence-mode",
        choices=("github-certification", "circleci-iteration", "circleci-certification"),
        default="github-certification",
        help=(
            "github-certification preserves the GitHub-hosted evidence gate; "
            "circleci-iteration is private diagnostic evidence; "
            "circleci-certification is restricted to the exact integration candidate"
        ),
    )
    args = parser.parse_args()
    if args.evidence_mode == "github-certification":
        if (
            os.environ.get("GITHUB_ACTIONS") != "true"
            or os.environ.get("RUNNER_ENVIRONMENT") != "github-hosted"
        ):
            parser.error(
                "GitHub certification mode is restricted to GitHub-hosted Actions"
            )
        root = Path("verification/composition")
        evidence_authority = "github-hosted-certification"
        certification_eligible = True
    else:
        if os.environ.get("CIRCLECI") != "true":
            parser.error(
                "CircleCI evidence mode requires the real CircleCI environment; "
                "do not fake CI provider variables"
            )
        if os.environ.get("GITHUB_ACTIONS") == "true":
            parser.error("CircleCI evidence mode cannot run inside GitHub Actions")
        if args.evidence_mode == "circleci-certification":
            head = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
            if (
                os.environ.get("CIRCLE_BRANCH") != "integration/composition-av"
                or os.environ.get("SEMWRIGHT_CIRCLECI_CANDIDATE_CERTIFICATION") != "true"
                or os.environ.get("CIRCLE_SHA1") != head
            ):
                parser.error(
                    "CircleCI certification requires the exact integration/composition-av "
                    "candidate and explicit certification mode"
                )
            root = Path("verification/circleci-certification")
            evidence_authority = "circleci-hosted-candidate-certification"
            certification_eligible = True
        else:
            root = Path("verification/circleci-composition")
            evidence_authority = "circleci-private-iteration"
            certification_eligible = False
    suite = SUITES[args.suite]
    packages = suite["packages"]
    minimum = suite_minimum(args.suite)
    cmd = ["cargo", "test", "--locked"]
    for package in packages:
        cmd.extend(["-p", package])
    cmd.extend(suite["targets"])
    cmd.extend(["--", "--nocapture"])
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
        "evidence_mode": args.evidence_mode,
        "evidence_authority": evidence_authority,
        "certification_eligible": certification_eligible,
        "github_sha": os.environ.get("GITHUB_SHA"),
        "run_id": os.environ.get("GITHUB_RUN_ID") or os.environ.get("CIRCLE_WORKFLOW_ID"),
        "run_attempt": os.environ.get("GITHUB_RUN_ATTEMPT"),
        "job": os.environ.get("GITHUB_JOB") or os.environ.get("CIRCLE_JOB"),
        "runner": os.environ.get("RUNNER_OS") or "Linux/CircleCI",
        "github_run_id": os.environ.get("GITHUB_RUN_ID"),
        "github_run_attempt": os.environ.get("GITHUB_RUN_ATTEMPT"),
        "github_job": os.environ.get("GITHUB_JOB"),
        "circle_sha1": os.environ.get("CIRCLE_SHA1"),
        "circle_workflow_id": os.environ.get("CIRCLE_WORKFLOW_ID"),
        "circle_job": os.environ.get("CIRCLE_JOB"),
        "rustc": subprocess.check_output(["rustc", "--version"], text=True).strip(),
        "lock_sha256": hashlib.sha256(Path("Cargo.lock").read_bytes()).hexdigest(),
        "exit_code": result.returncode,
        "expected_minimum": minimum,
        "passed": passed, "failed": failed, "ignored": ignored,
        "status": "PASS" if ok else "FAIL",
        "evidence_scope": (
            "portable unit/contract tests, not native application acceptance"
            if certification_eligible
            else "private CircleCI iteration diagnostic; not certification evidence"
        ),
    }
    (root / (args.suite + ".json")).write_text(json.dumps(report, indent=2) + "\n")
    if not ok:
        print("suite failed or ran fewer tests than required", file=sys.stderr)
        return 1
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
