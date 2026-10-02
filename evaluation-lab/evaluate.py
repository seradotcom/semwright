"""Result validation and paired analysis; never authors or repairs native projects.

No model calls, runtime launches, secret discovery, or product mutation occur here.
The deterministic harness tests are not agent-productivity measurements.
"""
import hashlib
import itertools
import json
import random
import re
from pathlib import Path

ARMS = ("direct", "low_level", "semantic")
STUDIES = ("P", "S")
OUTCOMES = ("COMPLETED", "FAILED", "BLOCKED", "ROUTE_VIOLATION")
PHASES = ("create", "appearance", "rule", "replace_source", "derived", "recovery")
SHA = re.compile(r"[0-9a-f]{40}\Z")
DIGEST = re.compile(r"[0-9a-f]{64}\Z")
MEASUREMENTS = ("queue_ms", "setup_ms", "runtime_ms", "render_ms", "model_ms",
                "user_wall_ms", "cpu_ms", "gpu_ms", "input_tokens", "output_tokens",
                "billed_cost_microunits", "model_calls", "tool_calls", "native_mutations",
                "repairs", "retries", "human_interventions", "app_crashes")


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"),
                      ensure_ascii=False, allow_nan=False).encode()


def digest(value):
    return hashlib.sha256(canonical(value)).hexdigest()


def load(path):
    path = Path(path)
    if path.is_symlink() or not path.is_file() or path.stat().st_size > 1_048_576:
        raise ValueError("Expected a bounded regular evidence file")
    def unique_object(items):
        result = {}
        for key, value in items:
            if key in result:
                reject("Duplicate evidence key")
            result[key] = value
        return result
    return json.loads(path.read_text(), object_pairs_hook=unique_object,
                      parse_constant=lambda _: reject("Non-finite JSON"))


def reject(message):
    raise ValueError(message)


def validate_registry(registry):
    if registry.get("schema_version") != 1 or registry.get("split") not in ("public_dev", "heldout"):
        reject("Registry version/split is invalid")
    tasks = registry.get("tasks", [])
    if not 10 <= len(tasks) <= 100:
        reject("Five families require at least two varied instances each")
    ids = set()
    families = {name: [] for name in ("godot", "blender", "cross_app", "media", "recovery")}
    for task in tasks:
        if task.get("id") in ids or not isinstance(task.get("id"), str):
            reject("Task identities must be unique")
        ids.add(task["id"])
        if task.get("family") not in families:
            reject("Unknown task family")
        families[task["family"]].append(digest(task["parameters"]))
        if task.get("phases") != list(PHASES):
            reject("Every task needs creation and five registered revisions")
        for field in ("requirements", "preserved_resources", "input_allowances", "native_oracles"):
            if not isinstance(task.get(field), list) or not task[field]:
                reject("Task acceptance and input allowances are required")
    if any(len(set(items)) < 2 for items in families.values()):
        reject("Families need distinct parameters, not duplicate task labels")
    return registry


def validate_freeze(freeze, registry):
    validate_registry(registry)
    if freeze.get("schema_version") != 1 or freeze.get("technical_gate") != "PASS":
        reject("A technically blocked product cannot become an evaluation freeze")
    for field in ("source_sha", "suite_sha"):
        if not SHA.fullmatch(freeze.get(field, "")):
            reject("Full immutable source and suite SHAs are required")
    for field in ("task_digest", "baseline_helpers_digest", "model_config_digest",
                  "runtime_manifest_digest", "skills_manifest_digest"):
        if not DIGEST.fullmatch(freeze.get(field, "")):
            reject("Freeze dependencies must all be pinned by digest")
    if freeze["task_digest"] != digest(registry) or registry["split"] != "heldout":
        reject("The final study must use the frozen heldout registry")
    if freeze.get("model_access_authorized") is not True or not freeze.get("model_identity"):
        reject("Comparable authorized model sessions are required")
    if freeze.get("arms") != list(ARMS) or freeze.get("studies") != list(STUDIES):
        reject("Required arms/studies cannot disappear after results are observed")
    seeds = freeze.get("seeds", [])
    if not seeds or len(seeds) != len(set(seeds)) or any(type(s) is not int for s in seeds):
        reject("Replica seeds must be explicit and unique")
    if freeze.get("failure_propagation") != "remaining_phases_failed":
        reject("Creation failure treatment must be preregistered")
    budget = freeze.get("budget", {})
    for field in ("max_model_tokens", "max_tool_calls", "max_attempt_wall_ms"):
        if type(budget.get(field)) is not int or budget[field] <= 0:
            reject("Every attempt requires fixed positive budgets")
    return freeze


def schedule(freeze, registry):
    validate_freeze(freeze, registry)
    rows = []
    for study, seed, task in itertools.product(STUDIES, freeze["seeds"], registry["tasks"]):
        arms = list(ARMS)
        rng = random.Random(digest([freeze["task_digest"], study, seed, task["id"]]))
        rng.shuffle(arms)
        for order, arm in enumerate(arms):
            rows.append({"task_id": task["id"], "study": study, "seed": seed,
                         "arm": arm, "order": order, "phases": list(PHASES)})
    return rows


def validate_attempt(attempt, freeze, registry):
    validate_freeze(freeze, registry)
    if attempt.get("schema_version") != 1 or attempt.get("execution_kind") != "MODEL_AGENT":
        reject("Deterministic scripts cannot become agent benchmark results")
    for field in ("source_sha", "suite_sha", "task_digest", "model_config_digest"):
        if attempt.get(field) != freeze[field]:
            reject("Attempt belongs to another target, suite, task split or model configuration")
    key = tuple(attempt.get(k) for k in ("task_id", "study", "seed", "arm"))
    if key not in {tuple(r[k] for k in ("task_id", "study", "seed", "arm"))
                   for r in schedule(freeze, registry)}:
        reject("Unregistered attempt")
    if attempt.get("model_identity") != freeze["model_identity"]:
        reject("Model identity differs across arms")
    if attempt.get("outcome") not in OUTCOMES:
        reject("Unrecognized outcome")
    metrics = attempt.get("metrics", {})
    for name in MEASUREMENTS:
        entry = metrics.get(name)
        if not isinstance(entry, dict) or set(entry) != {"value", "reason"}:
            reject("Every cost denominator needs a value or explicit unknown reason")
        value = entry["value"]
        if value is None:
            if not isinstance(entry["reason"], str) or not entry["reason"].strip():
                reject("Unobserved costs must not be fabricated or silently set to zero")
        elif type(value) is not int or value < 0 or entry["reason"] is not None:
            reject("Measured costs must be nonnegative integers with declared units")
    phases = attempt.get("phases", [])
    task = next(task for task in registry["tasks"] if task["id"] == attempt["task_id"])
    if [p.get("name") for p in phases] != list(PHASES):
        reject("Missing, reordered or duplicate revision results")
    for phase in phases:
        if phase.get("outcome") not in OUTCOMES or not isinstance(phase.get("checks"), list):
            reject("Phase outcome/checks are malformed")
        if phase["outcome"] == "COMPLETED":
            checks = phase["checks"]
            names = [c.get("name") for c in checks]
            if len(set(names)) != len(names) or set(names) != set(task["native_oracles"]):
                reject("All task-specific native oracles must be accounted for")
            if not checks or any(c.get("outcome") != "PASS" or c.get("native") is not True
                                 or not DIGEST.fullmatch(c.get("evidence_sha256", "")) for c in checks):
                reject("Completion requires independent native evidence for every required check")
    if attempt["outcome"] == "COMPLETED" and any(p["outcome"] != "COMPLETED" for p in phases):
        reject("Partial tasks do not count as complete")
    if phases[0]["outcome"] != "COMPLETED" and any(p["outcome"] == "COMPLETED" for p in phases[1:]):
        reject("Later phases cannot inherit another arm's successful base")
    actions = attempt.get("actions")
    if not isinstance(actions, list):
        reject("Authorized action trace is required")
    def allowed_mutation(action):
        if action.get("origin") == "broker":
            return action.get("actor") in ("model", "deterministic_backend")
        return (action.get("origin") == "declared_fault"
                and action.get("actor") == "harness"
                and action.get("phase") == "recovery"
                and action.get("fault_id") in task.get("declared_faults", []))
    violations = [a for a in actions if a.get("authoring_mutation") is True
                  and not allowed_mutation(a)]
    if attempt["study"] == "S" and attempt["arm"] == "semantic" and violations:
        if attempt["outcome"] != "ROUTE_VIOLATION":
            reject("Model-authored files outside semantic capabilities must be reported")
    if attempt["outcome"] == "COMPLETED" and not actions:
        reject("An empty trace cannot substantiate semantic or direct success")
    return attempt


def aggregate(attempts, freeze, registry):
    expected = {(r["task_id"], r["study"], r["seed"], r["arm"]) for r in schedule(freeze, registry)}
    rows = {}
    for attempt in attempts:
        validate_attempt(attempt, freeze, registry)
        key = tuple(attempt[k] for k in ("task_id", "study", "seed", "arm"))
        if key in rows:
            reject("Duplicate/best-of attempts are not allowed")
        rows[key] = attempt
    if set(rows) != expected:
        reject("Final analysis is incomplete; blocked and failed attempts still need records")
    counts = {study: {arm: {o: 0 for o in OUTCOMES} for arm in ARMS} for study in STUDIES}
    pairs = []
    for row in rows.values():
        counts[row["study"]][row["arm"]][row["outcome"]] += 1
    for task, study, seed in sorted({key[:3] for key in expected}):
        direct = rows[(task, study, seed, "direct")]
        semantic = rows[(task, study, seed, "semantic")]
        a, b = direct["metrics"]["user_wall_ms"]["value"], semantic["metrics"]["user_wall_ms"]["value"]
        pairs.append({"task_id": task, "study": study, "seed": seed,
                      "direct_outcome": direct["outcome"], "semantic_outcome": semantic["outcome"],
                      "semantic_minus_direct_user_wall_ms": None if a is None or b is None else b-a})
    return {"source_sha": freeze["source_sha"], "suite_sha": freeze["suite_sha"],
            "execution_kind": "MODEL_AGENT", "attempts": len(rows), "counts": counts,
            "paired_all_attempts": pairs, "winner_claim": None,
            "statistical_significance_claimed": False, "r16_closed": False}
