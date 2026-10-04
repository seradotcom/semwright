#!/usr/bin/env python3
"""Allowlisted exact-source diagnostics executed by the hosted CI lane."""
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time
if os.environ.get("GITHUB_ACTIONS") != "true":
    raise SystemExit("Project Graph suites require GitHub Actions")
root = Path(__file__).resolve().parents[2]
os.chdir(root)
selection = json.loads(Path("scripts/project-graph/lane.json").read_text())
if set(selection) != {"suite"} or selection["suite"] not in {"contracts", "store", "lockfile", "fuzz-lock", "full"}:
    raise SystemExit("unknown Project Graph suite")
if len(sys.argv) != 2 or sys.argv[1] not in {"auto", selection["suite"]}:
    raise SystemExit("suite request does not match committed selector")
suite = selection["suite"]
out = root / "verification/project-graph"
out.mkdir(parents=True, exist_ok=True)
sha = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
if sha != (os.environ.get("EXPECTED_SHA") or os.environ["GITHUB_SHA"]):
    raise SystemExit("checkout does not match expected source SHA")
start = time.monotonic()
report = {"schema_version": 1, "role": "project-graph", "source_sha": sha, "workflow_sha": os.environ["GITHUB_SHA"], "contract_sha": "26602e4b25929be869d69ef28fef4dd9713180d7", "workflow": os.environ.get("GITHUB_WORKFLOW"), "run_id": os.environ.get("GITHUB_RUN_ID"), "attempt": os.environ.get("GITHUB_RUN_ATTEMPT"), "job": os.environ.get("GITHUB_JOB"), "event": os.environ.get("GITHUB_EVENT_NAME"), "suite": suite, "scope": "lock-resolution-only" if suite == "lockfile" else "portable-model-not-native", "native": False, "outcome": "UNKNOWN", "requested_tests": 0, "executed_tests": 0, "ignored_tests": 0, "job_id": None, "runtime_versions": {}, "steps": []}
def run(name, command, print_output=True, check=True):
    report["active_step"] = name
    result = subprocess.run(command, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    (out / (name + ".log")).write_text(result.stdout)
    if print_output:
        print(result.stdout, flush=True)
    report["steps"].append({"name": name, "command": command, "exit_code": result.returncode})
    if result.returncode and check:
        raise RuntimeError(name + " failed")
    return result.stdout
try:
    report["runtime_versions"]["rustc"] = subprocess.check_output(["rustc", "--version"], text=True).strip()
    report["lock_sha256_before"] = hashlib.sha256(Path("Cargo.lock").read_bytes()).hexdigest()
    if suite == "fuzz-lock":
        run("fuzz-lock", [sys.executable, "scripts/project-graph/fuzz-lane.py", "resolve"])
        report["outcome"] = "LOCKFILE_RESOLVED_NOT_TESTED"
    elif suite == "lockfile":
        run("resolve-lock", ["cargo", "metadata", "--format-version", "1"], False)
        (out / "Cargo.lock").write_bytes(Path("Cargo.lock").read_bytes())
        report["outcome"] = "LOCKFILE_RESOLVED_NOT_TESTED"
        # Cargo metadata includes runner paths, not useful final evidence.
        (out / "resolve-lock.log").unlink()
    else:
        features = ["--features", "store"] if suite in {"store", "full"} else []
        package = ["--locked", "-p", "semwright-project-graph"]
        run("format", ["cargo", "fmt", "-p", "semwright-project-graph", "--", "--check"])
        inventory = run("inventory", ["cargo", "test", *package, *features, "--all-targets", "--", "--list"])
        expected_tests = len(re.findall(r"^.+: test$", inventory, re.MULTILINE))
        report["requested_tests"] = expected_tests
        if expected_tests < (36 if suite in {"store", "full"} else 29):
            raise RuntimeError("missing graph test inventory")
        tests = run("tests", ["cargo", "test", *package, *features, "--all-targets"], check=False)
        summaries = re.findall(r"^test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out;", tests, re.MULTILINE)
        passed = sum(int(x[0]) for x in summaries)
        failed = sum(int(x[1]) for x in summaries)
        ignored = sum(int(x[2]) for x in summaries)
        report.update(executed_tests=passed + failed, ignored_tests=ignored, failed_tests=failed)
        if report["steps"][-1]["exit_code"] or failed or ignored or passed != expected_tests:
            raise RuntimeError("executed passing inventory does not match request")
        run("clippy", ["cargo", "clippy", *package, *features, "--all-targets", "--", "-D", "warnings"])
        schemas = run("schemas", ["cargo", "run", "--quiet", *package, *features, "--example", "schemas"], False)
        json.loads(schemas)
        (out / "schemas.json").write_text(schemas)
        run("rustdoc", ["cargo", "doc", *package, *features, "--no-deps"])
        if suite == "full":
            run(
                "integration-format",
                [
                    "cargo", "fmt",
                    "-p", "semwright-core",
                    "-p", "semwright-registry",
                    "-p", "semwright-platform-services",
                    "-p", "semwright-daemon",
                    "--", "--check",
                ],
            )
            broker_inventory = run(
                "broker-inventory",
                ["cargo", "test", "--locked", "-p", "semwright-core", "--test", "project_graph", "--", "--list"],
            )
            broker_expected = len(re.findall(r"^.+: test$", broker_inventory, re.MULTILINE))
            report["broker_requested_tests"] = broker_expected
            if broker_expected < 10:
                raise RuntimeError("missing Broker Project Graph test inventory")
            broker_tests = run(
                "broker-tests",
                ["cargo", "test", "--locked", "-p", "semwright-core", "--test", "project_graph"],
                check=False,
            )
            broker_summaries = re.findall(
                r"^test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;",
                broker_tests,
                re.MULTILINE,
            )
            broker_passed = sum(int(row[0]) for row in broker_summaries)
            broker_failed = sum(int(row[1]) for row in broker_summaries)
            broker_ignored = sum(int(row[2]) for row in broker_summaries)
            report["broker_executed_tests"] = broker_passed + broker_failed
            if (
                report["steps"][-1]["exit_code"]
                or broker_failed
                or broker_ignored
                or broker_passed != broker_expected
            ):
                raise RuntimeError("Broker Project Graph tests failed or were skipped")
            registry_rebuild = run(
                "registry-rebuild-test",
                [
                    "cargo", "test", "--locked",
                    "-p", "semwright-registry",
                    "tests::preparation_relationship_is_host_owned_pinned_and_provider_bound",
                    "--", "--exact",
                ],
                check=False,
            )
            registry_summaries = re.findall(
                r"^test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;",
                registry_rebuild,
                re.MULTILINE,
            )
            registry_passed = sum(int(row[0]) for row in registry_summaries)
            registry_failed = sum(int(row[1]) for row in registry_summaries)
            registry_ignored = sum(int(row[2]) for row in registry_summaries)
            if (
                report["steps"][-1]["exit_code"]
                or registry_failed
                or registry_ignored
                or registry_passed != 1
            ):
                raise RuntimeError("trusted rebuild Registry regression did not pass exactly once")
            report["registry_rebuild_tests"] = registry_passed
            rebuild_broker = run(
                "rebuild-broker-test",
                [
                    "cargo", "test", "--locked",
                    "-p", "semwright-core",
                    "--test", "provider_runtime",
                    "rebuild_preparation_relation_reenters_broker_and_invalidates_on_refresh",
                    "--", "--exact",
                ],
                check=False,
            )
            rebuild_summaries = re.findall(
                r"^test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;",
                rebuild_broker,
                re.MULTILINE,
            )
            rebuild_passed = sum(int(row[0]) for row in rebuild_summaries)
            rebuild_failed = sum(int(row[1]) for row in rebuild_summaries)
            rebuild_ignored = sum(int(row[2]) for row in rebuild_summaries)
            if (
                report["steps"][-1]["exit_code"]
                or rebuild_failed
                or rebuild_ignored
                or rebuild_passed != 1
            ):
                raise RuntimeError("typed rebuild Broker regression did not pass exactly once")
            report["rebuild_broker_tests"] = rebuild_passed
            run(
                "integration-clippy-registry",
                ["cargo", "clippy", "--locked", "-p", "semwright-registry", "--lib", "--", "-D", "warnings"],
            )
            run(
                "integration-clippy-core-rebuild",
                [
                    "cargo", "clippy", "--locked",
                    "-p", "semwright-core",
                    "--test", "provider_runtime",
                    "--", "-D", "warnings",
                ],
            )
            principal_tests = run(
                "principal-tests",
                [
                    "cargo", "test", "--locked",
                    "-p", "semwright-platform-services",
                    "durable_user_principal_excludes_broker_or_logon_session_identity",
                ],
                check=False,
            )
            principal_summaries = re.findall(
                r"^test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;",
                principal_tests,
                re.MULTILINE,
            )
            principal_passed = sum(int(row[0]) for row in principal_summaries)
            principal_failed = sum(int(row[1]) for row in principal_summaries)
            if report["steps"][-1]["exit_code"] or principal_failed or principal_passed != 1:
                raise RuntimeError("durable OS principal regression did not execute exactly once")
            report["principal_tests"] = principal_passed
            run("daemon-wiring", ["cargo", "check", "--locked", "-p", "semwright-daemon"])
            run("bounded-fuzz", [sys.executable, "scripts/project-graph/fuzz-lane.py", "run"])
        report["outcome"] = "PASS"
except Exception as error:
    report.update(outcome="FAIL", error=str(error))
    raise
finally:
    report["lock_sha256"] = hashlib.sha256(Path("Cargo.lock").read_bytes()).hexdigest()
    report["duration_seconds"] = round(time.monotonic() - start, 3)
    report["artifacts"] = {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in out.iterdir() if p.is_file() and p.name != "evidence.json"}
    (out / "evidence.json").write_text(json.dumps(report, indent=2) + "\n")
