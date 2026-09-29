"""Oracle controls and meaningful guard mutants, never product acceptance."""
from __future__ import annotations
import inspect
import json
import os
import sys
from pathlib import Path
import lab_core as core

SOURCE = "1" * 40
SUITE = "2" * 40

def row(case_id="G-SELF-001", outcome="PASS"):
    return {"case_id": case_id, "source_sha": SOURCE, "suite_sha": SUITE,
            "outcome": outcome, "scope": "product_contract", "isolation_verified": True}

def rejects(call):
    try:
        call()
    except core.EvidenceError:
        return True
    return False

def summary(rows=None, requested=None, evaluator=core.summarize, **kwargs):
    return evaluator(["G-SELF-001"] if requested is None else requested,
                     [row()] if rows is None else rows, SOURCE, SUITE, **kwargs)

def changed(**kwargs):
    result = row()
    result.update(kwargs)
    return result

def probe(**kwargs):
    value = {"schema_version": 1, "case_id": "G-SELF-001", "source_sha": SOURCE, "observed": {"accepted": False}}
    value.update(kwargs)
    return value

def mutant(needle, replacement):
    text = inspect.getsource(core.summarize)
    if text.count(needle) != 1:
        raise RuntimeError("oracle mutant no longer matches exact source")
    namespace = dict(vars(core))
    exec(compile(text.replace(needle, replacement), "<lab-oracle-mutant>", "exec"), namespace)
    return namespace["summarize"]

def run_all():
    tests = [
        ("G-SELF-001", lambda: summary()["status"] == "NO_OPEN_BLOCKING_FINDINGS_IN_TESTED_SCOPE"),
        ("G-SELF-002", lambda: rejects(lambda: summary([], []))),
        ("G-SELF-003", lambda: rejects(lambda: summary([]))),
        ("G-SELF-004", lambda: rejects(lambda: summary([row(), row()]))),
        ("G-SELF-005", lambda: rejects(lambda: summary([changed(source_sha="3" * 40)]))),
        ("G-SELF-006", lambda: rejects(lambda: summary([changed(suite_sha="3" * 40)]))),
        ("G-SELF-007", lambda: rejects(lambda: summary([changed(scope="native_application")]))),
        ("G-SELF-008", lambda: rejects(lambda: summary([changed(isolation_verified=False)]))),
        ("G-SELF-009", lambda: rejects(lambda: summary([changed(outcome="SKIPPED")]))),
        ("G-SELF-010", lambda: summary([row(outcome="FAIL")])["status"] == "AUDIT_COMPLETE_WITH_FINDINGS"),
        ("G-SELF-011", lambda: summary([row(outcome="BLOCKED")])["status"] == "BLOCKED"),
        ("G-SELF-012", lambda: summary([row(outcome="NOT_RUN")])["status"] == "BLOCKED"),
        ("G-SELF-013", lambda: summary(open_blockers=1)["status"] == "AUDIT_COMPLETE_WITH_FINDINGS"),
        ("G-SELF-014", lambda: rejects(lambda: core.strict_json('{"a":{"x":1,"x":2}}'))),
        ("G-SELF-015", lambda: all(rejects(lambda s=s: core.strict_json(s)) for s in ["NaN", "Infinity", "-Infinity"])),
        ("G-SELF-016", lambda: rejects(lambda: core.strict_json('PASS: all tests passed'))),
        ("G-SELF-017", lambda: rejects(lambda: core.validate_probe(probe(case_id="G-SELF-999"), "G-SELF-001", SOURCE))),
        ("G-SELF-018", lambda: rejects(lambda: core.validate_probe(probe(source_sha="main"), "G-SELF-001", SOURCE))),
        ("G-SELF-019", lambda: rejects(lambda: core.validate_probe(probe(schema_version=True), "G-SELF-001", SOURCE))),
        ("G-SELF-020", lambda: not core.compare_observation({"n": True}, {"n": 1})),
        ("G-SELF-021", lambda: not core.compare_observation({}, {"measurement": None})),
        ("G-SELF-022", lambda: not core.compare_observation(float("nan"), float("nan"))),
        ("G-SELF-023", lambda: rejects(lambda: summary([row()], ["G-SELF-001", "G-SELF-002"]))),
        ("G-SELF-024", lambda: not rejects(lambda: summary([], evaluator=mutant("if seen != set(requested):", "if False:")))),
        ("G-SELF-025", lambda: not rejects(lambda: summary([changed(source_sha="3" * 40)], evaluator=mutant(
            'if row.get("source_sha") != source_sha or row.get("suite_sha") != suite_sha:', 'if False:')))),
        ("G-SELF-026", lambda: summary([row(outcome="FAIL")], evaluator=mutant(
            'elif counts["FAIL"] or open_blockers:', 'elif open_blockers:'))["status"] != "AUDIT_COMPLETE_WITH_FINDINGS"),
        ("G-SELF-027", lambda: rejects(lambda: core.validate_retest(
            {"finding_id": "G-F-001", "outcome": "FAIL", "source_sha": SOURCE},
            {"finding_id": "G-F-001", "outcome": "PASS", "source_sha": SUITE, "affected_cases": ["a"], "run_id": "1"},
            SUITE, {"a"}))),
        ("G-SELF-028", lambda: rejects(lambda: core.full_sha("a" * 7))),
        ("G-SELF-029", lambda: rejects(lambda: core.strict_json('"' + 'x' * 1_048_577 + '"'))),
        ("G-SELF-030", lambda: summary()["native_acceptance"] is False and summary()["r16_closed"] is False),
    ]
    results = []
    for case_id, test in tests:
        try:
            ok = test() is True
            error = None
        except Exception as exc:
            ok = False
            error = type(exc).__name__
        results.append({"case_id": case_id, "ok": ok, "error_class": error})
    return results

if __name__ == "__main__":
    if os.environ.get("HOME") != "/home/lab" or not Path("/canary/readonly").is_file():
        raise SystemExit("disposable enclosure required")
    results = run_all()
    print(json.dumps({"schema_version": 1, "results": results}, allow_nan=False))
    sys.exit(0 if all(row["ok"] for row in results) else 1)
