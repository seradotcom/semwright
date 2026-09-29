#!/usr/bin/env python3
"""Allowlisted exact-SHA lanes. Actual tests execute only on disposable runners."""
from __future__ import annotations
import argparse
import datetime
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import tempfile
from isolation import Enclosure, require_hosted
from lab_core import EvidenceError, LANES, digest, full_sha, strict_json, summarize, write_json

LAB = Path(__file__).resolve().parent
OWNERS = {"composition": "A", "av": "A", "motion": "A", "figma": "A", "audio": "B"}

def git(root: Path, *args: str) -> str:
    return subprocess.check_output(["git", "-C", str(root), *args], text=True).strip()

def config():
    target = strict_json((LAB / "targets.json").read_bytes())
    if type(target["schema_version"]) is not int or target["schema_version"] != 1:
        raise EvidenceError("unknown target-lock version")
    for sha in target["targets"].values():
        full_sha(sha)
    full_sha(target["baseline_sha"])
    full_sha(target["contract_sha"])
    lanes = target["selected_lanes"]
    if not lanes or len(set(lanes)) != len(lanes) or any(x not in LANES for x in lanes):
        raise EvidenceError("invalid lane selector")
    return target

def metadata(source_sha: str, suite_sha: str, lane: str) -> dict:
    return {"schema_version": 1, "role": "G", "lane": lane, "source_sha": source_sha,
            "suite_sha": suite_sha, "contract_sha": config()["contract_sha"],
            "product_target_sha": None if lane == "selftest" else source_sha,
            "dependency_shas": config()["targets"], "workflow": os.environ.get("GITHUB_WORKFLOW"),
            "github_sha": os.environ.get("GITHUB_SHA"), "run_id": os.environ.get("GITHUB_RUN_ID"),
            "run_attempt": os.environ.get("GITHUB_RUN_ATTEMPT"), "job_key": os.environ.get("GITHUB_JOB"),
            "job_id": None, "job_id_reason": "REST database ID enriched after run; unavailable in job environment",
            "event": os.environ.get("GITHUB_EVENT_NAME"), "runner_os": platform.platform(),
            "runner_image": {"os": os.environ.get("ImageOS"), "version": os.environ.get("ImageVersion")},
            "runtime_versions": {"python": platform.python_version()}, "features": [],
            "timestamp_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
            "limits": config()["limits"], "combined_candidate_sha": config()["combined_candidate_sha"]}

def selftest(source_sha: str, suite_sha: str, cases: list[dict], report: dict):
    enclosure = Enclosure(LAB, source_sha)
    try:
        report["isolation"] = enclosure.preflight()
        historical = config()["harness_history"]
        full_sha(historical["suite_sha"])
        if historical["path"] != "tests/semantic-adversarial-lab/lab_core.py":
            raise EvidenceError("historical source path outside G lab")
        blob = subprocess.check_output(["git", "-C", str(LAB.parents[1]), "show",
                                        historical["suite_sha"] + ":" + historical["path"]])
        if digest(blob) != historical["sha256"]:
            raise EvidenceError("historical oracle source digest mismatch")
        with tempfile.TemporaryDirectory(prefix="g-history-", dir=os.environ["RUNNER_TEMP"]) as directory:
            (Path(directory) / "before_lab_core.py").write_bytes(blob)
            raw = enclosure.run(["/usr/bin/python3", "/lab/selftest.py"], source=Path(directory))
        report["harness_history"] = {**historical, "fix_suite_sha": suite_sha,
                                     "before_controls": ["G-SELF-053", "G-SELF-054"],
                                     "after_controls": ["G-SELF-031", "G-SELF-042", "G-SELF-043"],
                                     "scope": "lab oracle defects only; not product vulnerabilities"}
        parsed = strict_json(raw["stdout"])
        if set(parsed) != {"schema_version", "results"} or type(parsed["schema_version"]) is not int or parsed["schema_version"] != 1:
            raise EvidenceError("selftest receipt missing")
        expected = [c["id"] for c in cases]
        received = [r["case_id"] for r in parsed["results"]]
        if received != expected:
            raise EvidenceError("selftest case set/count/order mismatch")
        results = []
        for row in parsed["results"]:
            if set(row) != {"case_id", "ok", "error_class"} or type(row["ok"]) is not bool:
                raise EvidenceError("malformed selftest row")
            results.append({"case_id": row["case_id"], "source_sha": source_sha, "suite_sha": suite_sha,
                            "scope": "lab_selftest", "isolation_verified": enclosure.verified,
                            "outcome": "PASS" if row["ok"] and raw["exit_code"] == 0 else "FAIL",
                            "error_class": row["error_class"]})
        report["results"] = results
        report["execution"] = {k: v for k, v in raw.items() if k not in {"stdout", "stderr"}}
        if raw["termination_reason"] or not raw["canaries_unchanged"] or not raw["outer_process_group_gone"]:
            raise EvidenceError("test enclosure lifecycle failed")
        return results
    finally:
        report["cleanup_verified"] = enclosure.close()

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("lane", choices=sorted(LANES | {"matrix"}))
    parser.add_argument("--output", type=Path)
    parser.add_argument("--target-checkout", type=Path)
    args = parser.parse_args()
    lock = config()
    if args.lane == "matrix":
        print(json.dumps({"include": [{"lane": lane, "target_sha": lock["targets"][OWNERS.get(lane, "main")]} for lane in lock["selected_lanes"]]}))
        return 0
    require_hosted()
    root = LAB.parents[1]
    suite_sha = full_sha(git(root, "rev-parse", "HEAD"))
    if suite_sha != os.environ.get("GITHUB_SHA"):
        raise EvidenceError("suite checkout differs from event SHA")
    if git(root, "status", "--porcelain", "--untracked-files=no"):
        raise EvidenceError("lab tracked files changed after checkout")
    cases = [c for c in strict_json((LAB / "registry.json").read_bytes())["cases"] if c["lane"] == args.lane]
    if not cases:
        raise EvidenceError("zero selected tests is a failure, not success")
    source_sha = suite_sha if args.lane == "selftest" else lock["targets"][OWNERS.get(args.lane, "main")]
    report = metadata(source_sha, suite_sha, args.lane)
    output = args.output or Path(os.environ["RUNNER_TEMP"]) / "g-lab-evidence" / (args.lane + ".json")
    requested = [c["id"] for c in cases]
    scope = cases[0]["scope"]
    try:
        if args.lane == "selftest":
            results = selftest(source_sha, suite_sha, cases, report)
        else:
            if args.target_checkout is None:
                raise EvidenceError("BLOCKED: immutable target checkout required")
            if git(args.target_checkout, "rev-parse", "HEAD") != source_sha:
                raise EvidenceError("target SHA mismatch")
            if git(args.target_checkout, "status", "--porcelain", "--untracked-files=no"):
                raise EvidenceError("target has tracked modifications")
            from product import run_product
            results = run_product(args.lane, args.target_checkout.resolve(), source_sha, suite_sha, cases, report)
        report["results"] = results
        report["summary"] = summarize(requested, results, source_sha, suite_sha, scope=scope)
        if report.get("cleanup_verified") is not True:
            raise EvidenceError("cleanup not verified")
    except (EvidenceError, OSError, subprocess.SubprocessError) as exc:
        report["error"] = str(exc)
        partial = report.get("results", [])
        observed = {r["case_id"] for r in partial}
        report["results"] = partial + [{"case_id": case_id, "source_sha": source_sha, "suite_sha": suite_sha,
                              "scope": scope, "outcome": "BLOCKED", "isolation_verified": False,
                              "reason": str(exc)} for case_id in requested if case_id not in observed]
        report["summary"] = summarize(requested, report["results"], source_sha, suite_sha, scope=scope)
        report["summary"]["status"] = "BLOCKED"
        report["infrastructure_blockers"] = [str(exc)]
    report["requested_cases"] = requested
    report["skipped_cases"] = []
    report["limitations"] = ["Internal adversarial preparation, not independent R16 closure.",
                              "Results apply only to the explicit source and suite SHAs and declared cases.",
                              "No combined-wave acceptance until a candidate SHA is deliberately pinned."]
    write_json(output, report)
    print(json.dumps({"lane": args.lane, "source_sha": source_sha, "suite_sha": suite_sha,
                      "summary": report["summary"]}, sort_keys=True))
    return 0 if report["summary"]["status"] in {"HARNESS_SELFTEST_PASS", "NO_OPEN_BLOCKING_FINDINGS_IN_TESTED_SCOPE"} else 1

if __name__ == "__main__":
    sys.exit(main())
