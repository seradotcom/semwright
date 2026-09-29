"""Independent lab evidence rules; these are not a product authority or evaluator."""
from __future__ import annotations
import hashlib
import json
import math
import re
from pathlib import Path
from typing import Any

SCHEMA_VERSION = 1
SHA1 = re.compile(r"[0-9a-f]{40}\Z")
CASE_ID = re.compile(r"G-[A-Z]+-[0-9]{3}\Z")
OUTCOMES = frozenset({"PASS", "FAIL", "BLOCKED", "NOT_RUN"})
LANES = frozenset({"selftest", "composition", "audio", "av", "packaging", "graph", "godot-native", "blender-native", "motion", "figma", "lifecycle"})

class EvidenceError(ValueError):
    """Evidence is absent, ambiguous, stale, or outside the frozen contract."""

def _unique(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    out: dict[str, Any] = {}
    for key, value in pairs:
        if key in out:
            raise EvidenceError("duplicate JSON key")
        out[key] = value
    return out

def _finite_float(value: str) -> float:
    parsed = float(value)
    if not math.isfinite(parsed):
        raise EvidenceError("nonfinite JSON exponent")
    return parsed

def strict_json(data: str | bytes) -> Any:
    try:
        encoded = data.encode("utf-8", errors="strict") if isinstance(data, str) else data
        if len(encoded) > 1_048_576:
            raise EvidenceError("receipt exceeds byte budget")
        text = encoded.decode("utf-8", errors="strict")
        value = json.loads(text, object_pairs_hook=_unique, parse_float=_finite_float,
                           parse_constant=lambda _: (_ for _ in ()).throw(EvidenceError("nonfinite JSON")))
        pending = [(value, 0)]
        count = 0
        while pending:
            node, depth = pending.pop()
            count += 1
            if depth > 64 or count > 65536:
                raise EvidenceError("receipt nesting/node budget")
            if isinstance(node, str):
                node.encode("utf-8", errors="strict")
            elif isinstance(node, dict):
                pending.extend((k, depth + 1) for k in node)
                pending.extend((v, depth + 1) for v in node.values())
            elif isinstance(node, list):
                pending.extend((v, depth + 1) for v in node)
        return value
    except (ValueError, UnicodeError, RecursionError, OverflowError) as exc:
        raise EvidenceError("invalid strict JSON") from exc

def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()

def write_json(path: Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    encoded = json.dumps(value, indent=2, sort_keys=True, allow_nan=False) + "\n"
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(encoded, encoding="utf-8")
    temporary.replace(path)

def full_sha(value: Any) -> str:
    if not isinstance(value, str) or not SHA1.fullmatch(value):
        raise EvidenceError("full immutable Git SHA required")
    return value

def validate_probe(value: Any, expected_id: str, expected_sha: str) -> dict[str, Any]:
    if not isinstance(value, dict) or set(value) != {"schema_version", "case_id", "source_sha", "observed"}:
        raise EvidenceError("probe receipt field mismatch")
    if type(value["schema_version"]) is not int or value["schema_version"] != SCHEMA_VERSION:
        raise EvidenceError("probe schema mismatch")
    if value["case_id"] != expected_id or not CASE_ID.fullmatch(expected_id):
        raise EvidenceError("probe case substitution")
    if full_sha(value["source_sha"]) != full_sha(expected_sha):
        raise EvidenceError("probe source substitution")
    if not isinstance(value["observed"], dict):
        raise EvidenceError("probe observation is not an object")
    return value["observed"]

def compare_observation(actual: Any, expected: Any) -> bool:
    if type(actual) is not type(expected):
        return False
    if isinstance(expected, dict):
        return actual.keys() == expected.keys() and all(compare_observation(actual[k], v) for k, v in expected.items())
    if isinstance(expected, list):
        return len(actual) == len(expected) and all(compare_observation(a, b) for a, b in zip(actual, expected))
    if isinstance(expected, float) and (not math.isfinite(expected) or not math.isfinite(actual)):
        return False
    return actual == expected

def summarize(requested: list[str], results: list[dict[str, Any]], source_sha: str,
              suite_sha: str, *, open_blockers: int = 0, scope: str = "product_contract") -> dict[str, Any]:
    full_sha(source_sha)
    full_sha(suite_sha)
    if scope not in {"lab_selftest", "product_contract", "native_application", "source_inspection"}:
        raise EvidenceError("unknown evidence scope")
    if not isinstance(requested, list) or any(not isinstance(c, str) or not CASE_ID.fullmatch(c) for c in requested):
        raise EvidenceError("invalid requested case ID")
    if not isinstance(results, list) or any(not isinstance(row, dict) for row in results):
        raise EvidenceError("result family must contain objects")
    if not requested or len(requested) != len(set(requested)):
        raise EvidenceError("zero or duplicate requested cases")
    if type(open_blockers) is not int or open_blockers < 0:
        raise EvidenceError("invalid blocker count")
    seen: set[str] = set()
    counts = dict.fromkeys(sorted(OUTCOMES), 0)
    for row in results:
        if not isinstance(row.get("case_id"), str) or row.get("case_id") in seen or row.get("case_id") not in requested:
            raise EvidenceError("duplicate or unrequested result")
        seen.add(row["case_id"])
        if row.get("source_sha") != source_sha or row.get("suite_sha") != suite_sha:
            raise EvidenceError("mixed-SHA result family")
        outcome = row.get("outcome")
        if not isinstance(outcome, str) or outcome not in OUTCOMES:
            raise EvidenceError("unknown outcome")
        if row.get("scope") != scope:
            raise EvidenceError("evidence class/source spoof")
        if outcome == "PASS" and row.get("isolation_verified") is not True:
            raise EvidenceError("PASS without verified isolation")
        counts[outcome] += 1
    if seen != set(requested):
        raise EvidenceError("missing required case, including final page")
    if counts["BLOCKED"] or counts["NOT_RUN"]:
        status = "BLOCKED"
    elif counts["FAIL"] or open_blockers:
        status = "AUDIT_COMPLETE_WITH_FINDINGS"
    elif scope == "lab_selftest":
        status = "HARNESS_SELFTEST_PASS"
    else:
        status = "NO_OPEN_BLOCKING_FINDINGS_IN_TESTED_SCOPE"
    return {"status": status, "counts": counts, "scope": scope,
            "requested_count": len(requested), "executed_count": counts["PASS"] + counts["FAIL"],
            "open_blockers": open_blockers, "r16_closed": False,
            "native_acceptance": scope == "native_application" and status == "NO_OPEN_BLOCKING_FINDINGS_IN_TESTED_SCOPE"}

def validate_retest(before: dict[str, Any], after: dict[str, Any], fix_sha: str,
                    affected: set[str]) -> None:
    """A closure receipt is a complete exact-SHA family, not a caller PASS flag."""
    full_sha(fix_sha)
    if not isinstance(before, dict) or not isinstance(after, dict):
        raise EvidenceError("retest receipts must be objects")
    if before.get("finding_id") != after.get("finding_id") or not before.get("finding_id"):
        raise EvidenceError("finding identity mismatch")
    if before.get("outcome") != "FAIL" or after.get("outcome") != "PASS":
        raise EvidenceError("missing before-fail / after-pass")
    full_sha(before.get("source_sha"))
    if after.get("source_sha") != fix_sha or before.get("source_sha") == fix_sha:
        raise EvidenceError("fix SHA mismatch")
    full_sha(before.get("suite_sha"))
    if after.get("suite_sha") != before["suite_sha"]:
        raise EvidenceError("retest must use the same frozen oracle suite")
    if not isinstance(affected, set) or not affected or any(not isinstance(c, str) or not CASE_ID.fullmatch(c) for c in affected):
        raise EvidenceError("invalid affected family")
    for receipt in (before, after):
        declared = receipt.get("affected_cases")
        if not isinstance(declared, list) or any(not isinstance(c, str) for c in declared) or len(declared) != len(affected) or set(declared) != affected:
            raise EvidenceError("affected regression family not re-executed")
        for key in ("run_id", "job_id"):
            identifier = receipt.get(key)
            if isinstance(identifier, bool) or not re.fullmatch(r"[1-9][0-9]*", str(identifier)):
                raise EvidenceError("retest requires observed run and job IDs")
        proof_digest = receipt.get("evidence_sha256")
        if not isinstance(proof_digest, str) or not re.fullmatch(r"[0-9a-f]{64}", proof_digest):
            raise EvidenceError("retest needs a content-addressed evidence artifact")
        if receipt.get("cleanup_verified") is not True or receipt.get("infrastructure_blockers") != []:
            raise EvidenceError("retest infrastructure or cleanup unresolved")
        scope = receipt.get("scope")
        if scope not in {"product_contract", "native_application"}:
            raise EvidenceError("selftest/static inspection cannot close a product finding")
        summary = summarize(sorted(affected), receipt.get("results"), receipt["source_sha"],
                            receipt["suite_sha"], scope=scope)
        expected_status = "AUDIT_COMPLETE_WITH_FINDINGS" if receipt is before else "NO_OPEN_BLOCKING_FINDINGS_IN_TESTED_SCOPE"
        if summary["status"] != expected_status:
            raise EvidenceError("reported retest verdict disagrees with executed family")
    if before["scope"] != after["scope"]:
        raise EvidenceError("retest changed evidence class")
