#!/usr/bin/env python3
"""Narrow integration source diagnostic; executed exclusively on GitHub-hosted Actions."""
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "verification/semantic-creation"
EXPECTED_RECONCILIATION = {
    "reconciliation_preserves_unknown_ledger_and_authorizes_only_one_fresh_child",
    "reconciliation_never_refunds_operation_budget_or_accepts_foreign_owner",
    "reconciliation_rejects_foreign_vault_and_revoked_root_incarnation",
    "reconciliation_rejects_changed_binding_fixture_and_nonexhaustive_evidence",
    "observation_failure_reserves_budget_and_old_ticket_cannot_cross_new_read",
}
EXPECTED_D = {
    "uncertain_publication_reconciles_to_a_fresh_owner_bound_repair",
    "uncertain_publication_reconciliation_never_overwrites_a_human_edit",
}
EXPECTED_AUDIO = {
    "effect_contract_is_compiled_and_client_override_is_denied",
    "effect_consumer_preserves_decoder_provenance_and_never_claims_mutation_readback",
    "effect_consumer_leaves_unavailable_loudness_and_incomplete_decode_unknown",
    "effect_consumer_denies_foreign_channel_measurement_and_unapplied_verification",
    "gain_repair_keeps_the_shared_lifecycle_and_reverifies_new_pcm",
}


def run(name, command, expected):
    listed = subprocess.run(command + ["--", "--list"], cwd=ROOT, text=True,
                            stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    (OUT / (name + "-list.log")).write_text(listed.stdout)
    registered = {match.group(1) for match in re.finditer(r"^([^ ]+): test$", listed.stdout, re.M)}
    if listed.returncode or not expected <= registered:
        return {"outcome": "FAIL", "reason": "required regression absent", "registered": sorted(registered)}
    result = subprocess.run(command + ["--", "--nocapture"], cwd=ROOT, text=True,
                            stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    print(result.stdout, flush=True)
    (OUT / (name + ".log")).write_text(result.stdout)
    summaries = re.findall(r"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;", result.stdout)
    passed = sum(int(row[0]) for row in summaries)
    failed = sum(int(row[1]) for row in summaries)
    ignored = sum(int(row[2]) for row in summaries)
    executed = passed + failed
    completed = {match.group(1) for match in re.finditer(r"^test ([^ ]+) \.\.\. ok$", result.stdout, re.M)}
    ok = result.returncode == 0 and executed >= len(registered) and failed == 0 and ignored == 0 and expected <= completed
    return {"outcome": "PASS" if ok else "FAIL", "requested": len(registered),
            "executed": executed, "passed": passed, "failed": failed, "ignored": ignored,
            "expected_regressions": sorted(expected), "exit_code": result.returncode}


def main():
    if not os.environ.get("GITHUB_ACTIONS"):
        raise SystemExit("I diagnostics must run on GitHub-hosted Actions")
    OUT.mkdir(parents=True, exist_ok=True)
    sha = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    report = {"schema_version": 1, "role": "semantic-creation-integration", "source_sha": sha,
              "github_sha": os.environ.get("GITHUB_SHA"), "run_id": os.environ.get("GITHUB_RUN_ID"),
              "run_attempt": os.environ.get("GITHUB_RUN_ATTEMPT"),
              "lock_sha256": hashlib.sha256((ROOT / "Cargo.lock").read_bytes()).hexdigest(),
              "native_engine": False, "r16_closed": False, "outcome": "FAIL", "suites": {}}
    try:
        report["suites"]["a-contracts"] = run("a-contracts", ["cargo", "test", "--locked", "-p", "semwright-semantic-composition", "--all-targets"], EXPECTED_RECONCILIATION)
        report["suites"]["d-profile"] = run("d-profile", ["cargo", "test", "--locked", "-p", "semwright-driver-godot", "--test", "authoring_profile"], EXPECTED_D)
        report["suites"]["audio-consumer"] = run("audio-consumer", ["cargo", "test", "--locked", "-p", "semwright-audio-authoring", "--all-targets"], EXPECTED_AUDIO)
        report["suites"]["effect-contracts"] = run("effect-contracts", ["cargo", "test", "--locked", "-p", "semwright-effect-conformance", "--all-targets"], set())
        report["outcome"] = "PASS" if all(s["outcome"] == "PASS" for s in report["suites"].values()) else "FAIL"
    finally:
        (OUT / "reconciliation.json").write_text(json.dumps(report, indent=2) + "\n")
    return 0 if report["outcome"] == "PASS" else 1


if __name__ == "__main__":
    raise SystemExit(main())
