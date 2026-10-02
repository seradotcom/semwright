#!/usr/bin/env python3
"""Create a deterministic, bounded G source/evidence backup; no test execution."""
from __future__ import annotations
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import zipfile

LAB = Path(__file__).resolve().parent
REPO = LAB.parents[1]
WORKFLOW = ".github/workflows/semantic-adversarial-lab.yml"
PREFIX = "tests/semantic-adversarial-lab/"
MAX_SOURCE = 8 * 1024 * 1024

def git(*args: str) -> bytes:
    return subprocess.check_output(["git", "-C", str(REPO), *args])

def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()

def allowed(path: str) -> bool:
    return (path.startswith(PREFIX) or path == WORKFLOW) and not any(x in {"..", ".git", "target", "__pycache__", "node_modules"} for x in path.split("/"))

def encoded(value) -> bytes:
    return (json.dumps(value, indent=2, sort_keys=True, allow_nan=False) + "\n").encode()

def evidence_matches_lock(index: dict, lock: dict) -> bool:
    return (
        index.get("targets") == lock.get("targets")
        and index.get("combined_candidate_sha") == lock.get("combined_candidate_sha")
    )

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--commit", required=True)
    parser.add_argument("--evidence-dir", type=Path, action="append", default=[])
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    commit = git("rev-parse", args.commit + "^{commit}").decode().strip()
    if len(args.commit) != 40 or args.commit != commit:
        raise SystemExit("Full immutable delivery SHA required")
    lock = json.loads(git("show", commit + ":" + PREFIX + "targets.json"))
    baseline = lock["baseline_sha"]
    changed = git("diff", "--name-only", baseline, commit).decode().splitlines()
    if not changed or not all(allowed(name) for name in changed):
        raise SystemExit("Backup includes a product change or is empty")
    bundle = {}
    for record in git("ls-tree", "-rz", commit, "--", PREFIX, WORKFLOW).split(b"\0"):
        if not record:
            continue
        header, raw_name = record.split(b"\t", 1)
        mode, kind, _ = header.decode().split()
        name = raw_name.decode("utf-8", errors="strict")
        if not allowed(name) or kind != "blob" or mode not in {"100644", "100755"}:
            raise SystemExit("Nonregular or out-of-scope source in backup")
        blob = git("show", commit + ":" + name)
        if len(blob) > 1024 * 1024:
            raise SystemExit("Unexpected large G source file")
        bundle["source/" + name] = blob
    patch = git("diff", "--binary", "--full-index", baseline, commit, "--", PREFIX, WORKFLOW)
    if len(patch) > 4 * 1024 * 1024:
        raise SystemExit("Patch byte budget exceeded")
    bundle["patches/G-from-baseline.patch"] = patch
    bundle["patches/COMMIT_LOG.txt"] = git("log", "--reverse", "--format=%H %s", baseline + ".." + commit)
    experiments = []
    for directory in args.evidence_dir:
        directory = directory.resolve()
        index_path = directory / "EXPERIMENT_INDEX.json"
        if index_path.is_symlink() or not index_path.is_file():
            raise SystemExit("Missing regular experiment index")
        index = json.loads(index_path.read_bytes())
        if index.get("role") != "G" or index.get("repo") != "seradotcom/semwright":
            raise SystemExit("Evidence is not from G's authorized repository")
        if not evidence_matches_lock(index, lock):
            raise SystemExit(
                "Historical target or combined candidate differs; do not mix experiments in this backup"
            )
        run_id = index.get("run_id")
        if type(run_id) is not int or run_id < 1:
            raise SystemExit("Invalid run ID")
        suite = index.get("suite_sha", "")
        if len(suite) != 40 or git("rev-parse", suite + "^{commit}").decode().strip() != suite:
            raise SystemExit("Unknown experiment suite")
        experiments.append({"run_id": run_id, "run_attempt": index.get("run_attempt"),
                            "suite_sha": suite, "source_shas": index["targets"],
                            "combined_candidate_sha": index.get("combined_candidate_sha"),
                            "state": index.get("status"), "observed_at": index.get("observed_at"),
                            "known_executed_count": index.get("known_executed_count"),
                            "unknown_execution_lanes": index.get("unknown_execution_lanes"),
                            "current_delivery_suite": suite == commit})
        for name in ("EXPERIMENT_INDEX.json", "JOB_PROVENANCE.json", "EVIDENCE.md"):
            path = directory / name
            if path.is_symlink() or not path.is_file() or path.stat().st_size > 1024 * 1024:
                raise SystemExit("Missing, nonregular or oversized evidence summary")
            key = f"evidence/{run_id}/{name}"
            if key in bundle:
                raise SystemExit("Duplicate experiment in backup")
            bundle[key] = path.read_bytes()
    registry = json.loads(bundle["source/" + PREFIX + "registry.json"])["cases"]
    manifest = {"schema_version": 1, "role": "G", "kind": "SOURCE_CHECKPOINT_NOT_AUDIT_ACCEPTANCE",
                "repository": "seradotcom/semwright", "pull_request": 174,
                "baseline_sha": baseline, "delivery_commit": commit,
                "contract_sha": lock["contract_sha"], "targets": lock["targets"],
                "combined_candidate_sha": lock["combined_candidate_sha"],
                "registered_cases": len(registry), "experiments": experiments,
                "product_source_changes": [], "native_acceptance": False, "r16_closed": False,
                "full_wave_readiness": "BLOCKED", "patch_sha256": sha(patch),
                "files": {name: {"bytes": len(data), "sha256": sha(data)} for name, data in sorted(bundle.items())},
                "exclusions": [".git", "targets/caches", "binary builds", "real credentials", "user native projects", "private untriaged reproducer details"],
                "limitations": ["Registered cases are not executed tests.",
                                "Separate product targets do not certify an integrated candidate.",
                                "No independent R16 or native acceptance follows from this source package.",
                                "Private findings remain with owners until coordinated disclosure."]}
    bundle["MANIFEST.json"] = encoded(manifest)
    bundle["SHA256SUMS"] = "".join(f"{sha(data)}  {name}\n" for name, data in sorted(bundle.items())).encode()
    if len(bundle) > 256 or sum(map(len, bundle.values())) > MAX_SOURCE:
        raise SystemExit("Backup source/count budget exceeded")
    out = args.output.resolve()
    if out.exists():
        raise SystemExit("Use a new backup path; do not overwrite a delivered artifact")
    out.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(out, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
        for name, data in sorted(bundle.items()):
            info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            info.create_system = 3
            info.external_attr = 0o100644 << 16
            info.compress_type = zipfile.ZIP_DEFLATED
            archive.writestr(info, data)
    with zipfile.ZipFile(out) as archive:
        if archive.namelist() != sorted(bundle):
            raise SystemExit("Created backup entry mismatch")
        for name, data in bundle.items():
            if sha(archive.read(name)) != sha(data):
                raise SystemExit("Created backup hash mismatch")
    archive_sha = sha(out.read_bytes())
    out.with_suffix(out.suffix + ".sha256").write_text(archive_sha + "  " + out.name + "\n")
    print(json.dumps({"path": str(out), "bytes": out.stat().st_size, "sha256": archive_sha,
                      "delivery_commit": commit, "files": len(bundle), "experiments": experiments}, indent=2))

if __name__ == "__main__":
    main()
