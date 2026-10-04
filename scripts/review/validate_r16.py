#!/usr/bin/env python3
"""Validate the bounded documentary/evidence surface, never authorize a release."""
from __future__ import annotations
import argparse
import hashlib
import json
from pathlib import Path
import re
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parents[2]
DOCS = ("README.md", "docs/installation.md", "docs/architecture.md", "docs/platforms.md",
        "SECURITY.md", "docs/security.md", "CONTRIBUTING.md", "docs/development.md",
        "docs/compatibility.md", "VERIFY.md", "RELEASE_BLOCKERS.md", "SUPPORT.md")
SHA = re.compile(r"[0-9a-f]{40}")


def duplicate_keys(pairs: list[tuple]) -> dict:
    obj = {}
    for key, value in pairs:
        if key in obj:
            raise ValueError(f"duplicate key: {key}")
        obj[key] = value
    return obj


def links(root: Path, relative: str) -> list[str]:
    path = root / relative
    if not path.is_file():
        return [f"missing document: {relative}"]
    errors = []
    text = re.sub(r"```.*?```", "", path.read_text(), flags=re.S)
    for value in re.findall(r"\[[^\]]*\]\(([^\s)]+)\)", text):
        parsed = urlsplit(value)
        if parsed.scheme or parsed.netloc or not parsed.path:
            continue
        target = (path.parent / unquote(parsed.path)).resolve()
        if not target.is_relative_to(root.resolve()) or not target.exists():
            errors.append(f"{relative}: missing/outside local link: {value}")
    return errors


def checksum_errors(root: Path = ROOT) -> list[str]:
    base = root / "verification/r16-closeout"
    sums = base / "SHA256SUMS"
    if not sums.is_file():
        return ["R16 SHA256SUMS is required"]
    errors = []
    seen = set()
    for line_no, raw in enumerate(sums.read_text().splitlines(), 1):
        if not raw.strip():
            continue
        parts = raw.split(None, 1)
        if len(parts) != 2 or not re.fullmatch(r"[0-9a-f]{64}", parts[0]):
            errors.append(f"SHA256SUMS:{line_no}: malformed checksum line")
            continue
        digest, relative = parts[0], parts[1].lstrip("* ")
        if relative in seen:
            errors.append(f"SHA256SUMS:{line_no}: duplicate path: {relative}")
            continue
        seen.add(relative)
        target = (base / relative).resolve()
        if not target.is_relative_to(base.resolve()):
            errors.append(f"SHA256SUMS:{line_no}: path escapes closeout root: {relative}")
            continue
        if not target.is_file():
            errors.append(f"SHA256SUMS:{line_no}: missing file: {relative}")
            continue
        actual = hashlib.sha256(target.read_bytes()).hexdigest()
        if actual != digest:
            errors.append(f"SHA256SUMS:{line_no}: digest mismatch: {relative}")
    return errors


def evidence_errors(record: dict, root: Path = ROOT) -> list[str]:
    errors = []
    for key in ("review_target_sha", "final_source_sha"):
        if not isinstance(record.get(key), str) or not SHA.fullmatch(record[key]):
            errors.append(f"missing full {key}")
    closed = record.get("r16_closed")
    if closed is True:
        receipt_path = root / "verification/r16-closeout/evidence/SEPARATE_REVALIDATION_2026-10-03.json"
        if not receipt_path.is_file():
            errors.append("R16 closure requires a separate revalidation receipt")
        else:
            receipt = json.loads(receipt_path.read_text(), object_pairs_hook=duplicate_keys)
            if receipt.get("external_audit") is not False or receipt.get("disposition", {}).get("R16") != "CLOSED":
                errors.append("R16 revalidation receipt does not support closure")
            if receipt.get("fix_sha") != "4ef9a06e486cd8d2e3851c298e244435ecef3232":
                errors.append("R16 revalidation receipt is not bound to the reviewed remediation")
    elif closed is not False:
        errors.append("r16_closed must be a boolean")
    if record.get("external_audit") is not False:
        errors.append("this R review is not an external audit")
    expected = {f"R16-{n:02}" for n in range(1, 13)}
    rows = record.get("areas", [])
    if len(rows) != 12 or {row.get("id") for row in rows} != expected:
        errors.append("exactly twelve distinct review areas required")
    for row in rows:
        if not row.get("limitations") or not row.get("source_locations"):
            errors.append(f"{row.get('id')}: explicit source locations and limitations required")
    for item in record.get("executions", []):
        if item.get("result") == "PASS" and (not item.get("run_id") or not SHA.fullmatch(item.get("source_sha", ""))):
            errors.append("execution PASS needs run identity and source SHA")
        if item.get("result") == "SKIPPED" and item.get("executed") is not False:
            errors.append("skipped execution cannot count as executed")
    return errors


def compact_evidence_errors(root: Path = ROOT) -> list[str]:
    base = root / "verification/r16-closeout"
    findings_path = base / "FINDINGS.json"
    source_path = base / "evidence/SOURCE_VALIDATION_2026-10-03.json"
    errors = []
    if not findings_path.is_file():
        errors.append("compact R16 findings ledger is required")
    if not source_path.is_file():
        errors.append("compact R16 source-validation record is required")
    if errors:
        return errors

    findings = json.loads(findings_path.read_text(), object_pairs_hook=duplicate_keys)
    rows = findings.get("findings", [])
    expected = {f"R-{n:03}" for n in range(1, 11)}
    if findings.get("schema_version") != 1 or len(rows) != 10:
        errors.append("R16 findings ledger must contain exactly ten findings")
    if {row.get("id") for row in rows} != expected:
        errors.append("R16 findings ledger IDs must be R-001 through R-010")
    r009 = next((row for row in rows if row.get("id") == "R-009"), None)
    if not r009 or not str(r009.get("status", "")).startswith("OPEN_"):
        errors.append("R-009 governance finding must remain explicit while unresolved")
    observation = findings.get("current_governance_observation", {})
    if observation.get("finding") != "R-009":
        errors.append("current governance observation must bind R-009")
    if observation.get("main_branch_protection") != "NONE":
        errors.append("recorded R-009 observation must preserve unprotected-main result")
    if observation.get("repository_rulesets") != []:
        errors.append("recorded R-009 observation must preserve empty ruleset result")
    if not SHA.fullmatch(str(observation.get("observed_against_main_sha", ""))):
        errors.append("R-009 current observation needs an exact main SHA")

    source = json.loads(source_path.read_text(), object_pairs_hook=duplicate_keys)
    if source.get("schema_version") != 1 or source.get("result") != "PASS_IN_RECORDED_SCOPE":
        errors.append("R16 source-validation record has an unsupported disposition")
    for key in ("review_target_sha", "final_source_sha", "final_source_tree", "product_fix_sha"):
        if not SHA.fullmatch(str(source.get(key, ""))):
            errors.append(f"R16 source-validation needs full {key}")
    executions = source.get("executions", [])
    if len(executions) != 4:
        errors.append("R16 source-validation must retain four durable executions")
    for item in executions:
        if item.get("result") != "PASS":
            errors.append("retained R16 execution must preserve its PASS result")
        if not isinstance(item.get("run_id"), int) or item["run_id"] <= 0:
            errors.append("retained R16 execution needs run_id")
        if not isinstance(item.get("job_id"), int) or item["job_id"] <= 0:
            errors.append("retained R16 execution needs job_id")
        if not SHA.fullmatch(str(item.get("source_sha", ""))):
            errors.append("retained R16 execution needs exact source SHA")
        if re.fullmatch(r"[0-9a-f]{64}", str(item.get("log_sha256", ""))) is None:
            errors.append("retained R16 execution needs log SHA-256")
    return errors


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--require-report", action="store_true")
    args = parser.parse_args()
    errors = [error for name in DOCS for error in links(ROOT, name)]
    install = (ROOT / "docs/installation.md").read_text()
    for block in re.findall(r"```sh\n(.*?)```", install, re.S):
        if "scripts/dev/bootstrap.sh" in block:
            errors.append("ordinary installation must not invoke lock-initialization bootstrap")
    if not (ROOT / "Cargo.lock").is_file():
        errors.append("committed lockfile is required")
    record = ROOT / "verification/r16-closeout/R16_EVIDENCE_MANIFEST.json"
    if record.exists():
        errors.extend(evidence_errors(json.loads(record.read_text(), object_pairs_hook=duplicate_keys)))
    elif args.require_report:
        errors.append("review evidence manifest absent")
    errors.extend(checksum_errors(ROOT))
    errors.extend(compact_evidence_errors(ROOT))
    print(json.dumps({"validator": "r16-documentary-surface", "result": "FAIL" if errors else "PASS",
                      "documents_checked": len(DOCS), "errors": errors,
                      "external_links_checked": False, "security_verdict": False}, indent=2))
    return 1 if errors else 0


if __name__ == "__main__":
    raise SystemExit(main())
