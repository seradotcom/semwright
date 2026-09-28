#!/usr/bin/env python3
"""Validate and summarize benchmark evidence without executing application operations."""
from __future__ import annotations
import argparse
import copy
import hashlib
import json
from pathlib import Path
import re
import sys

HERE = Path(__file__).resolve().parent
CONFIG = json.loads((HERE / "benchmark-config.json").read_text())
SHA40 = re.compile(r"^[0-9a-f]{40}$")
SHA64 = re.compile(r"^[0-9a-f]{64}$")
ARMS = tuple(CONFIG["arms"])
STATUSES = {"NOT_RUN", "PASS", "FAIL", "BLOCKED"}
CHECKS = {"PASS", "FAIL", "UNKNOWN", "NOT_APPLICABLE"}

class InvalidEvidence(ValueError):
    pass

def require(condition: bool, message: str) -> None:
    if not condition:
        raise InvalidEvidence(message)

def digest_file(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()

def task(task_id: str) -> dict:
    for item in CONFIG["tasks"]:
        if item["id"] == task_id:
            return item
    raise InvalidEvidence(f"unknown benchmark task: {task_id}")

def validate(data: dict) -> dict:
    require(isinstance(data, dict), "evidence root must be an object")
    require(data.get("schema_version") == 1, "evidence schema_version must be 1")
    require(data.get("benchmark") == CONFIG["name"], "benchmark identity mismatch")
    spec = task(data.get("task_id", ""))
    tested_sha = data.get("tested_sha")
    require(tested_sha is None or SHA40.fullmatch(tested_sha), "tested_sha must be a full lowercase Git SHA")
    for field in ("input_digest", "output_profile_digest"):
        value = data.get(field)
        require(value is None or SHA64.fullmatch(value), f"{field} must be lowercase SHA-256")
    assets = data.get("asset_digests")
    require(isinstance(assets, list) and len(assets) <= 128, "asset_digests must be a bounded list")
    require(all(isinstance(value, str) and SHA64.fullmatch(value) for value in assets), "invalid asset digest")
    arms = data.get("arms")
    require(isinstance(arms, dict) and set(arms) == set(ARMS), "both configured benchmark arms are required")
    for arm_name in ARMS:
        arm = arms[arm_name]
        require(isinstance(arm, dict), f"{arm_name} must be an object")
        status = arm.get("status")
        require(status in STATUSES, f"{arm_name} has an invalid status")
        for metric in ("setup_ms", "execution_ms", "calls", "retries", "observable_tokens", "interventions"):
            value = arm.get(metric)
            require(value is None or isinstance(value, int) and value >= 0, f"{arm_name}.{metric} must be a nonnegative integer or null")
        checks = arm.get("checks")
        require(isinstance(checks, dict) and len(checks) <= 128, f"{arm_name}.checks must be bounded")
        require(all(value in CHECKS for value in checks.values()), f"{arm_name} has an invalid check state")
        artifacts = arm.get("artifact_digests")
        require(isinstance(artifacts, list) and len(artifacts) <= 128, f"{arm_name}.artifact_digests must be bounded")
        require(all(isinstance(value, str) and SHA64.fullmatch(value) for value in artifacts), f"{arm_name} has an invalid artifact digest")
        if status == "PASS":
            require(tested_sha is not None, f"{arm_name} PASS requires an exact tested SHA")
            require(data.get("input_digest") is not None and data.get("output_profile_digest") is not None, f"{arm_name} PASS requires input/profile digests")
            require(arm.get("verification") == "PASS", f"{arm_name} PASS requires fresh verification PASS")
            for check in spec["checks"]:
                require(checks.get(check) == "PASS", f"{arm_name} PASS is missing required check {check}")
            if spec["requires_native_editability"]:
                require(arm.get("native_editable") is True, f"{arm_name} PASS requires native editability")
    review = data.get("human_review")
    require(isinstance(review, dict) and isinstance(review.get("performed"), bool), "human_review must record performed")
    return {
        "valid": True,
        "task": spec["id"],
        "tested_sha": tested_sha,
        "config_sha256": digest_file(HERE / "benchmark-config.json"),
        "evidence_schema_sha256": digest_file(HERE / "evidence-schema.json"),
        "statuses": {name: arms[name]["status"] for name in ARMS},
        "winner": None,
    }

def delta(high, low):
    if isinstance(high, int) and isinstance(low, int):
        return high - low
    return None

def summarize(data: dict) -> dict:
    validation = validate(data)
    low = data["arms"]["low_level"]
    high = data["arms"]["high_level"]
    metrics = {}
    for name in ("setup_ms", "execution_ms", "calls", "retries", "observable_tokens", "interventions"):
        metrics[name] = {
            "low_level": low.get(name),
            "high_level": high.get(name),
            "high_minus_low": delta(high.get(name), low.get(name)),
        }
    return {
        "schema_version": 1,
        "benchmark": CONFIG["name"],
        "task_id": data["task_id"],
        "tested_sha": data.get("tested_sha"),
        "validation": validation,
        "metrics": metrics,
        "arms": {
            name: {
                "status": data["arms"][name]["status"],
                "verification": data["arms"][name].get("verification"),
                "native_editable": data["arms"][name].get("native_editable"),
                "checks": copy.deepcopy(data["arms"][name].get("checks", {})),
                "limitations": list(data["arms"][name].get("limitations", [])),
            }
            for name in ARMS
        },
        "human_review": copy.deepcopy(data["human_review"]),
        "interpretation": {
            "winner": None,
            "scores": None,
            "tool_calls_do_not_measure_visual_quality": True,
            "human_review_is_not_inferred": True,
        },
    }

def main() -> int:
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="command", required=True)
    v = sub.add_parser("validate")
    v.add_argument("evidence", type=Path)
    s = sub.add_parser("summarize")
    s.add_argument("evidence", type=Path)
    s.add_argument("output", type=Path)
    args = parser.parse_args()
    try:
        data = json.loads(args.evidence.read_text())
        if args.command == "validate":
            print(json.dumps(validate(data), indent=2, sort_keys=True))
        else:
            result = summarize(data)
            args.output.parent.mkdir(parents=True, exist_ok=True)
            args.output.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
            print(json.dumps({"written": str(args.output), "winner": None}, sort_keys=True))
        return 0
    except (OSError, json.JSONDecodeError, InvalidEvidence) as exc:
        print(f"benchmark evidence rejected: {exc}", file=sys.stderr)
        return 2

if __name__ == "__main__":
    raise SystemExit(main())
