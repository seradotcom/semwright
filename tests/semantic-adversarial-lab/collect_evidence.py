#!/usr/bin/env python3
"""Read-only, bounded GitHub evidence collection; never executes product/tests.

Artifacts are private triage material until disclosure is coordinated. Missing or
untrusted receipts are BLOCKED, not zero-test PASS. Run IDs and suite SHAs are
explicit inputs, never inferred from a moving branch or latest successful run.
"""
from __future__ import annotations
import argparse
import datetime
import json
from pathlib import Path
import subprocess
import sys
from artifact_io import MAX_ARCHIVE, read_evidence_archive
from oracle_identity import from_git as oracle_identity
from lab_core import EvidenceError, compare_observation, digest, full_sha, strict_json, summarize, write_json

REPO = "seradotcom/semwright"
LAB = Path(__file__).resolve().parent
ROLES = {"composition": "A", "av": "A", "motion": "A", "figma": "A", "audio": "B", "graph": "C", "effects": "F", "routing": "C", "godot-native": "D", "blender-native": "E", "lifecycle": "A"}

def target_for_lane(lock: dict, lane: str, suite: str) -> str:
    return suite if lane == "selftest" else lock["targets"][ROLES.get(lane, "main")]

def immutable_write(path: Path, data: bytes) -> None:
    """Content-addressed history is append-only, never an overwrite of a failure."""
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    if path.is_symlink():
        raise EvidenceError("history symlink prohibited")
    try:
        with path.open("xb") as stream:
            stream.write(data)
        path.chmod(0o600)
    except FileExistsError:
        if not path.is_file() or path.read_bytes() != data:
            raise EvidenceError("immutable evidence history collision/overwrite")


def preserve_old_aliases(output: Path, lanes: list[str]) -> None:
    names = ["EXPERIMENT_INDEX.json", "JOB_PROVENANCE.json", "FINDINGS_PRIVATE.json", "EVIDENCE.md"]
    names += [lane + ".json" for lane in lanes]
    for name in names:
        old = output / name
        if old.is_symlink():
            raise EvidenceError("evidence output alias is a symlink")
        if old.is_file():
            if old.stat().st_size > 1024 * 1024:
                raise EvidenceError("existing evidence exceeds preservation budget")
            data = old.read_bytes()
            immutable_write(output / "history" / "content" / digest(data) / name, data)


def api(endpoint: str, *, binary: bool = False):
    if not endpoint.startswith("repos/" + REPO + "/actions/"):
        raise EvidenceError("collector API outside the authorized lab repository")
    raw = subprocess.check_output(["gh", "api", endpoint], timeout=45)
    if len(raw) > MAX_ARCHIVE:
        raise EvidenceError("API response byte budget")
    return raw if binary else strict_json(raw)

def pages(endpoint: str, field: str) -> list[dict]:
    result = []
    for page in range(1, 11):
        payload = api(endpoint + f"?per_page=100&page={page}")
        result.extend(payload[field])
        if len(result) == payload["total_count"]:
            return result
        if not payload[field] or len(result) > payload["total_count"]:
            raise EvidenceError("incomplete or inconsistent API pagination")
    raise EvidenceError("API pagination budget exceeded")

def frozen_json(sha: str, filename: str):
    if filename not in {"targets.json", "registry.json"}:
        raise EvidenceError("collector source path is not a G manifest")
    raw = subprocess.check_output(["git", "-C", str(LAB.parents[1]), "show",
                                   sha + ":tests/semantic-adversarial-lab/" + filename])
    return strict_json(raw)

def validate_lane(report: dict, lane: str, cases: list[dict], lock: dict, suite: str, run: dict, *, expected_oracle: str | None = None):
    source = target_for_lane(lock, lane, suite)
    requested = [c["id"] for c in cases]
    scope = cases[0]["scope"]
    if (report.get("schema_version") != 1 or type(report.get("schema_version")) is not int
            or report.get("role") != "G" or report.get("lane") != lane
            or report.get("source_sha") != source or report.get("suite_sha") != suite
            or report.get("github_sha") != suite or str(report.get("run_id")) != str(run["id"])
            or str(report.get("run_attempt")) != str(run["run_attempt"])
            or report.get("requested_cases") != requested or report.get("skipped_cases") != []):
        raise EvidenceError("lane provenance or complete-case-set mismatch")
    if report.get("contract_sha") != lock["contract_sha"] or report.get("dependency_shas") != lock["targets"]:
        raise EvidenceError("contract/dependency substitution")
    if report.get("product_target_sha") != (None if lane == "selftest" else source):
        raise EvidenceError("selftest/product target attribution mismatch")
    if expected_oracle is not None and report.get("oracle_tree_sha256") is not None and report["oracle_tree_sha256"] != expected_oracle:
        raise EvidenceError("oracle identity differs from immutable suite source")
    calculated = summarize(requested, report.get("results"), source, suite, scope=scope)
    declared = report.get("summary", {})
    for field in ("counts", "scope", "requested_count", "executed_count", "r16_closed"):
        if not compare_observation(declared.get(field), calculated[field]):
            raise EvidenceError("reported counts/scope differ from independent recomputation")
    blocked = bool(report.get("infrastructure_blockers")) or report.get("cleanup_verified") is not True
    if declared.get("status") != calculated["status"] and not (blocked and declared.get("status") == "BLOCKED"):
        raise EvidenceError("reported verdict differs from independent recomputation")
    if declared.get("native_acceptance") is not calculated["native_acceptance"]:
        raise EvidenceError("reported native acceptance differs from independently recomputed scope/result")
    if blocked:
        calculated["status"] = "BLOCKED"
    return calculated

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--run-id", type=int, required=True)
    parser.add_argument("--suite-sha", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.run_id < 1:
        raise EvidenceError("positive run ID required")
    suite = full_sha(args.suite_sha)
    run = api(f"repos/{REPO}/actions/runs/{args.run_id}")
    if (run["head_sha"] != suite or run["head_branch"] != "test/semantic-adversarial-lab"
            or run.get("path") != ".github/workflows/semantic-adversarial-lab.yml"
            or run.get("event") not in {"push", "workflow_dispatch"}):
        raise EvidenceError("run does not belong to the explicit G workflow and suite")
    oracle = oracle_identity(LAB.parents[1], suite)
    lock = frozen_json(suite, "targets.json")
    registry = frozen_json(suite, "registry.json")["cases"]
    jobs = pages(f"repos/{REPO}/actions/runs/{args.run_id}/attempts/{run['run_attempt']}/jobs", "jobs")
    artifacts = pages(f"repos/{REPO}/actions/runs/{args.run_id}/artifacts", "artifacts")
    if sum(a["size_in_bytes"] for a in artifacts) > 8 * 1024 * 1024:
        raise EvidenceError("run artifact budget exceeded; do not download large build outputs")
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True, mode=0o700)
    output.chmod(0o700)
    if sum(p.stat().st_size for p in output.rglob("*") if p.is_file() and not p.is_symlink()) > 16 * 1024 * 1024:
        raise EvidenceError("per-run evidence retention budget reached; preserve/archive deliberately before more collection")
    preserve_old_aliases(output, lock["selected_lanes"])
    index = {"schema_version": 1, "role": "G", "repo": REPO, "suite_sha": suite,
             "run_id": run["id"], "run_attempt": run["run_attempt"], "run_url": run["html_url"],
             "oracle_tree_sha256": oracle, "collector_sha256": digest(Path(__file__).read_bytes()),
             "run_status": run["status"], "run_conclusion": run["conclusion"],
             "observed_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
             "targets": lock["targets"], "combined_candidate_sha": lock["combined_candidate_sha"],
             "lanes": [], "native_acceptance": False, "r16_closed": False}
    findings = []
    for lane in lock["selected_lanes"]:
        cases = [c for c in registry if c["lane"] == lane]
        matrix_target = target_for_lane(lock, lane, suite)
        if lane == "selftest":
            matrix_target = lock["targets"]["main"]
        job_matches = [j for j in jobs if j["name"] == f"boundary ({lane}, {matrix_target})"]
        artifact_matches = [a for a in artifacts if a["name"] == f"semantic-adversarial-{lane}-{suite}"]
        row = {"lane": lane, "requested_count": len(cases), "source_sha": suite if lane == "selftest" else matrix_target,
               "executed_count": None, "status": "BLOCKED", "counts": None,
               "job_id": job_matches[0]["id"] if len(job_matches) == 1 else None,
               "artifact_id": None, "receipt_sha256": None, "reason": None}
        try:
            if not cases or len(job_matches) != 1:
                raise EvidenceError("expected unique lane job/cases not yet available")
            job = job_matches[0]
            row["job_status"], row["job_conclusion"] = job["status"], job["conclusion"]
            if len(artifact_matches) != 1:
                if not job.get("steps") and job["status"] in {"queued", "waiting", "pending"}:
                    row["executed_count"] = 0
                raise EvidenceError("structured artifact unavailable; no inferred test PASS")
            artifact = artifact_matches[0]
            if artifact["expired"] or artifact["size_in_bytes"] > MAX_ARCHIVE:
                raise EvidenceError("artifact expired or exceeds download budget")
            remote_digest = artifact.get("digest") or ""
            if not remote_digest.startswith("sha256:"):
                raise EvidenceError("GitHub artifact SHA-256 unavailable")
            blob = api(f"repos/{REPO}/actions/artifacts/{artifact['id']}/zip", binary=True)
            files = read_evidence_archive(blob, lane, remote_digest[7:])
            raw = files[lane + ".json"]
            row.update(artifact_id=artifact["id"], artifact_sha256=digest(blob), receipt_sha256=digest(raw))
            immutable_write(output / "receipts" / (digest(raw) + ".json"), raw)
            report = strict_json(raw)
            calculated = validate_lane(report, lane, cases, lock, suite, run, expected_oracle=oracle)
            row.update(calculated)
            row["oracle_tree_sha256"] = oracle
            row.update(artifact_id=artifact["id"], artifact_sha256=digest(blob), receipt_sha256=digest(raw))
            if calculated["status"] in {"HARNESS_SELFTEST_PASS", "NO_OPEN_BLOCKING_FINDINGS_IN_TESTED_SCOPE"} and job["conclusion"] != "success":
                raise EvidenceError("green receipt without a successfully completed lane job")
            (output / (lane + ".json")).write_bytes(raw)
            case_map = {c["id"]: c for c in cases}
            for result in report["results"]:
                if result["outcome"] == "FAIL":
                    case = case_map[result["case_id"]]
                    findings.append({"id": "G-OBS-" + result["case_id"][2:], "case_id": result["case_id"],
                                     "owner": case["owner"], "target_sha": report["source_sha"],
                                     "suite_sha": suite, "run_id": run["id"], "job_id": job["id"],
                                     "state": "UNTRIAGED", "severity": None,
                                     "classification": "CONTRACT_FAILURE_OR_TEST_DEFECT_REQUIRES_OWNER_TRIAGE",
                                     "claim": case["description"], "expected": result.get("expected"),
                                     "observed": result.get("observed"), "evidence_sha256": digest(raw),
                                     "fix_sha": None, "retest": None, "public_disclosure_approved": False})
        except (EvidenceError, KeyError, TypeError, ValueError, subprocess.SubprocessError) as exc:
            row["status"] = "BLOCKED"
            row["reason"] = str(exc)
        index["lanes"].append(row)
    index["known_executed_count"] = sum(r["executed_count"] or 0 for r in index["lanes"])
    index["unknown_execution_lanes"] = [r["lane"] for r in index["lanes"] if r["executed_count"] is None]
    index["untriaged_failures"] = len(findings)
    index["status"] = ("BLOCKED" if any(r["status"] == "BLOCKED" for r in index["lanes"])
                       else "AUDIT_COMPLETE_WITH_FINDINGS" if findings else "NO_OPEN_BLOCKING_FINDINGS_IN_TESTED_SCOPE")
    index["full_wave_readiness"] = "BLOCKED"
    index["untested"] = ["combined I candidate", "Project Graph", "F E0 effects adapter", "Godot native",
                          "Blender native", "Figma collaborative native", "Motion native renders",
                          "audio devices/plugins/native apps", "full lifecycle fault injection", "product package/install boundaries"]
    write_json(output / "EXPERIMENT_INDEX.json", index)
    write_json(output / "FINDINGS_PRIVATE.json", {"schema_version": 1, "findings": findings})
    write_json(output / "JOB_PROVENANCE.json", {"run_id": run["id"], "attempt": run["run_attempt"],
        "jobs": [{k: j.get(k) for k in ("id", "name", "head_sha", "status", "conclusion", "started_at", "completed_at", "html_url", "steps")} for j in jobs]})
    lines = ["# G experiment evidence", "", f"Status: **{index['status']}**. Full-wave readiness: **BLOCKED**.",
             f"Suite: `{suite}`. Run: {run['id']}, attempt {run['run_attempt']}.", "",
             "| Lane | Requested | Known executed | State | Job |", "|---|---:|---:|---|---|"]
    for row in index["lanes"]:
        lines.append(f"| {row['lane']} | {row['requested_count']} | {row['executed_count'] if row['executed_count'] is not None else 'UNKNOWN'} | {row['status']} | {row['job_id']} |")
    lines += ["", "Only the declared exact-SHA contract/selftest cases are in scope. No native acceptance or R16 closure.",
              "Findings remain untriaged until impact/reachability and test-oracle correctness are reviewed with their owner.",
              "Missing receipts, incomplete execution and the absent integrated candidate remain explicit blockers."]
    (output / "EVIDENCE.md").write_text("\n".join(lines) + "\n")
    snapshot = output / "history" / ("attempt-" + str(run["run_attempt"])) / digest((output / "EXPERIMENT_INDEX.json").read_bytes())
    for name in ("EXPERIMENT_INDEX.json", "JOB_PROVENANCE.json", "FINDINGS_PRIVATE.json", "EVIDENCE.md"):
        immutable_write(snapshot / name, (output / name).read_bytes())
    print(json.dumps(index, indent=2))
    return 0

if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (EvidenceError, subprocess.SubprocessError) as error:
        print("BLOCKED evidence collection: " + str(error), file=sys.stderr)
        raise SystemExit(1)
