#!/usr/bin/env python3
"""Bounded, allowlisted libFuzzer smoke lane; GitHub-hosted runner only."""
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time
if os.environ.get("GITHUB_ACTIONS") != "true" or len(sys.argv) != 2 or sys.argv[1] not in {"resolve", "run"}:
    raise SystemExit("invalid remote fuzz-lane request")
root = Path(__file__).resolve().parents[2]
os.chdir(root)
fuzz = root / "scripts/project-graph/fuzz"
out = root / "verification/project-graph"
out.mkdir(parents=True, exist_ok=True)
sha = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
if sha != (os.environ.get("EXPECTED_SHA") or os.environ["GITHUB_SHA"]):
    raise SystemExit("fuzz source SHA mismatch")
report = {"version": 1, "source_sha": sha, "role": "C", "native_app_acceptance": False, "scope": "parser, declaration traversal and private synthetic store", "outcome": "UNKNOWN", "targets": []}
started = time.monotonic()
def execute(name, command, env=None):
    result = subprocess.run(command, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, env=env)
    (out / (name + ".log")).write_text(result.stdout)
    print(result.stdout[-20000:], flush=True)
    if result.returncode:
        raise RuntimeError(name + " failed")
    return result.stdout
try:
    if sys.argv[1] == "resolve":
        if not (fuzz / "Cargo.lock").exists():
            (fuzz / "Cargo.lock").write_bytes(Path("Cargo.lock").read_bytes())
        execute("fuzz-lock-resolution", ["cargo", "metadata", "--manifest-path", str(fuzz / "Cargo.toml"), "--format-version", "1"])
        (out / "Cargo-fuzz.lock").write_bytes((fuzz / "Cargo.lock").read_bytes())
        (out / "fuzz-lock-resolution.log").unlink()
        report["outcome"] = "LOCKFILE_RESOLVED_NOT_TESTED"
    else:
        expected_lock = hashlib.sha256((fuzz / "Cargo.lock").read_bytes()).hexdigest()
        execute("fuzz-dependencies", ["cargo", "fetch", "--locked", "--manifest-path", str(fuzz / "Cargo.toml")])
        report["engine"] = execute("fuzz-version", ["cargo", "+1.98.1", "fuzz", "--version"]).strip()
        if "0.13.2" not in report["engine"]:
            raise RuntimeError("unexpected cargo-fuzz version")
        report["toolchain"] = execute("fuzz-toolchain", ["rustc", "+nightly-2026-09-18", "--version"]).strip()
        clean_env = os.environ.copy()
        clean_env["CARGO_NET_OFFLINE"] = "true"
        for key in list(clean_env):
            if "TOKEN" in key or key.endswith("SECRET") or key in {"SSH_AUTH_SOCK", "GITHUB_EVENT_PATH"}:
                clean_env.pop(key, None)
        for target in ["manifest", "receipt", "traversal", "store"]:
            corpus = fuzz / "corpus" / target
            seeds = sorted(p for p in corpus.iterdir() if p.is_file())
            if not seeds:
                raise RuntimeError("fuzzer corpus is empty")
            seed_hashes = {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in seeds}
            command = ["cargo", "+nightly-2026-09-18", "fuzz", "run", target, "--fuzz-dir", str(fuzz), "--", "-max_total_time=15", "-timeout=3", "-rss_limit_mb=768", "-max_len=65536", "-seed=17", "-print_final_stats=1"]
            output = execute("fuzz-" + target, command, clean_env)
            counts = re.findall(r"^stat::number_of_executed_units:\s+(\d+)\s*$", output, re.MULTILINE)
            if not counts or int(counts[-1]) == 0:
                raise RuntimeError("fuzzer did not report positive executed input count")
            report["targets"].append({"target": target, "executions": int(counts[-1]), "seed_sha256": seed_hashes, "max_seconds": 15, "timeout_seconds": 3, "rss_limit_mb": 768, "seed": 17, "outcome": "PASS"})
            if hashlib.sha256((fuzz / "Cargo.lock").read_bytes()).hexdigest() != expected_lock:
                raise RuntimeError("fuzz build changed the committed dependency lock")
        report["outcome"] = "PASS"
except Exception as error:
    report.update(outcome="FAIL", error=str(error))
    raise
finally:
    report["duration_seconds"] = round(time.monotonic() - started, 3)
    if (fuzz / "Cargo.lock").exists():
        report["lock_sha256"] = hashlib.sha256((fuzz / "Cargo.lock").read_bytes()).hexdigest()
    (out / "fuzz-evidence.json").write_text(json.dumps(report, indent=2) + "\n")
