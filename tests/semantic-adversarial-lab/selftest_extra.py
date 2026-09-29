"""G's additional oracle controls. Execute only inside the hosted enclosure."""
from __future__ import annotations
import copy
import importlib.util
from pathlib import Path
import lab_core as core

BEFORE = "1" * 40
FIX = "3" * 40
SUITE = "2" * 40
FAMILY = {"G-PLAN-001", "G-PLAN-002"}

def rejects(call):
    try:
        call()
    except core.EvidenceError:
        return True
    return False

def receipts():
    pair = []
    for source, failing, run in [(BEFORE, True, "17"), (FIX, False, "18")]:
        pair.append({"finding_id": "G-F-001", "outcome": "FAIL" if failing else "PASS",
                     "source_sha": source, "suite_sha": SUITE, "oracle_tree_sha256": "b" * 64, "scope": "product_contract",
                     "affected_cases": sorted(FAMILY), "run_id": run, "job_id": run + "01",
                     "evidence_sha256": "a" * 64, "cleanup_verified": True, "infrastructure_blockers": [],
                     "results": [{"case_id": case_id, "source_sha": source, "suite_sha": SUITE,
                                  "scope": "product_contract", "isolation_verified": True,
                                  "outcome": "FAIL" if failing and i == 0 else "PASS"}
                                 for i, case_id in enumerate(sorted(FAMILY))]})
    return pair

def closure_rejects(change):
    before, after = copy.deepcopy(receipts())
    change(before, after)
    return rejects(lambda: core.validate_retest(before, after, FIX, FAMILY))

def positive_closure():
    core.validate_retest(*receipts(), FIX, FAMILY)
    return True

def historic_core():
    path = Path("/source/before_lab_core.py")
    spec = importlib.util.spec_from_file_location("g_historic_core", path)
    if spec is None or spec.loader is None:
        raise RuntimeError("historical source not provisioned")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module

def historical_nonfinite_observed():
    import math
    return math.isinf(historic_core().strict_json('{"n":1e9999}')["n"])

def historical_unobserved_closure_observed():
    historical = historic_core()
    before = {"finding_id": "G-F-001", "outcome": "FAIL", "source_sha": BEFORE}
    after = {"finding_id": "G-F-001", "outcome": "PASS", "source_sha": FIX,
             "affected_cases": sorted(FAMILY), "run_id": "18", "job_id": "1801"}
    historical.validate_retest(before, after, FIX, FAMILY)
    return rejects(lambda: core.validate_retest(before, after, FIX, FAMILY))

def target_only_retest():
    before, after = receipts()
    after["suite_sha"] = "4" * 40
    for result in after["results"]:
        result["suite_sha"] = after["suite_sha"]
    core.validate_retest(before, after, FIX, FAMILY)
    return True

def extra_cases():
    return [
        ("G-SELF-031", lambda: rejects(lambda: core.strict_json('{"n":1e9999}'))),
        ("G-SELF-032", lambda: rejects(lambda: core.strict_json('{"n":-1e9999}'))),
        ("G-SELF-033", lambda: rejects(lambda: core.strict_json('{"x":1}'.encode("utf-16")))),
        ("G-SELF-034", lambda: rejects(lambda: core.strict_json(b'"\\ud800"'))),
        ("G-SELF-035", lambda: rejects(lambda: core.strict_json(b'{"\\udfff":1}'))),
        ("G-SELF-036", lambda: rejects(lambda: core.strict_json("[" * 65 + "0" + "]" * 65))),
        ("G-SELF-037", lambda: core.strict_json("[" * 64 + "0" + "]" * 64) is not None),
        ("G-SELF-038", lambda: rejects(lambda: core.summarize(["G-SELF-038"], [], BEFORE, SUITE, scope="invented"))),
        ("G-SELF-039", lambda: rejects(lambda: core.summarize(["PASS"], [], BEFORE, SUITE))),
        ("G-SELF-040", lambda: rejects(lambda: core.summarize(["G-SELF-040"], [], BEFORE, SUITE, open_blockers=True))),
        ("G-SELF-041", lambda: rejects(lambda: core.summarize(["G-SELF-041"], ["PASS"], BEFORE, SUITE))),
        ("G-SELF-042", lambda: closure_rejects(lambda b, a: a.pop("results"))),
        ("G-SELF-043", positive_closure),
        ("G-SELF-044", lambda: closure_rejects(lambda b, a: a["results"].pop())),
        ("G-SELF-045", lambda: closure_rejects(lambda b, a: a.update(suite_sha="4" * 40, oracle_tree_sha256="c" * 64))),
        ("G-SELF-046", lambda: closure_rejects(lambda b, a: a.update(evidence_sha256="not-a-hash"))),
        ("G-SELF-047", lambda: closure_rejects(lambda b, a: a["results"][0].update(source_sha=BEFORE))),
        ("G-SELF-048", lambda: closure_rejects(lambda b, a: a["results"][0].update(outcome="BLOCKED"))),
        ("G-SELF-049", lambda: core.strict_json('{"title":"caf\u00e9 \u96ea"}') == {"title":"caf\u00e9 \u96ea"}),
        ("G-SELF-050", lambda: closure_rejects(lambda b, a: a.update(cleanup_verified=False))),
        ("G-SELF-051", lambda: closure_rejects(lambda b, a: a.update(job_id=True))),
        ("G-SELF-052", lambda: closure_rejects(lambda b, a: a["results"][0].update(isolation_verified=False))),
        ("G-SELF-053", historical_nonfinite_observed),
        ("G-SELF-054", historical_unobserved_closure_observed),
    ]
