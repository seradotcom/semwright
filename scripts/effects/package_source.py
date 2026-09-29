#!/usr/bin/env python3
"""Reproducible source-only backup. No build, native runtime or test execution.
The chosen immutable Git source supplies every packaged byte, never a dirty tree.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parents[2]
OWNED = ("crates/effect-conformance/", "scripts/effects/", "docs/effects/")
WORKFLOW = ".github/workflows/effect-conformance.yml"
A_CONTRACT = "26602e4b25929be869d69ef28fef4dd9713180d7"
DEFAULT_BASE = "6ee52b428310370d3ad438a13964086a63f48367"


def git(*args):
    return subprocess.check_output(["git", "-C", str(ROOT), *args])


def sha(data):
    return hashlib.sha256(data).hexdigest()


def build(source, base, output):
    for value in (source, base):
        if not re.fullmatch(r"[a-f0-9]{40}", value):
            raise ValueError("source and base must be full immutable SHA-1 commit IDs")
        git("cat-file", "-e", value + "^{commit}")
    if git("rev-parse", source + ":crates/semantic-composition") != git("rev-parse", A_CONTRACT + ":crates/semantic-composition"):
        raise ValueError("A shared contract changed; review and version the backup dependency first")
    if git("rev-parse", source + ":crates/project-graph") != git("rev-parse", base + ":crates/project-graph"):
        raise ValueError("C P0 changed; review the backup dependency first")
    candidates = git("ls-tree", "-r", "--name-only", source, "--", *OWNED, WORKFLOW).decode().splitlines()
    files = {}
    for name in sorted(candidates):
        if name.startswith("docs/effects/backup/"):
            continue  # Never recursively archive a prior backup.
        if ".." in Path(name).parts or name.startswith("/"):
            raise ValueError("unsafe source path")
        data = git("show", source + ":" + name)
        data.decode("utf-8")
        if len(data) > 1_048_576:
            raise ValueError("source file budget exceeded")
        files[name] = data
    if not files or len(files) > 256 or sum(map(len, files.values())) > 2_097_152:
        raise ValueError("source bundle budget exceeded")
    selected = [*files, "Cargo.lock"]
    patch = git("diff", "--no-ext-diff", "--no-textconv", "--binary", "--full-index", base, source, "--", *selected)
    lock = git("show", source + ":Cargo.lock")
    with tempfile.TemporaryDirectory(prefix="F-patch-reconstruction-") as temp:
        root = Path(temp)
        for name in selected:
            found = subprocess.run(["git", "-C", str(ROOT), "show", base + ":" + name], stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
            if found.returncode == 0:
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(found.stdout)
        subprocess.run(["git", "init", "--quiet", str(root)], check=True)
        subprocess.run(["git", "-C", str(root), "-c", "core.autocrlf=false", "apply", "--check", "-"], input=patch, check=True)
        subprocess.run(["git", "-C", str(root), "-c", "core.autocrlf=false", "apply", "-"], input=patch, check=True)
        for name, expected in {**files, "Cargo.lock": lock}.items():
            if (root / name).read_bytes() != expected:
                raise ValueError("restored source mismatch: " + name)
    manifest = {"schema_version": 1, "role": "F", "source_sha": source,
                "patch_base_sha": base, "contract_sha": A_CONTRACT,
                "main_baseline_sha": "b736d41b61c4a4146c9e75c16796e251b025e69f",
                "files": {name: sha(data) for name, data in files.items()},
                "lock_sha256": sha(lock), "patch_sha256": sha(patch),
                "reconstruction": "git apply --check and byte-for-byte source comparison passed",
                "build_or_native_execution": False,
                "acceptance": "NOT_ESTABLISHED_BY_PACKAGING"}
    readme = f"""# F source backup — not an accepted release

This is a reconstructive patch ZIP, not a full repository checkout. F_SOURCE.patch contains all {len(files)} F-owned implementation, contract, test, native harness, CI and documentation files plus the Cargo.lock delta. SOURCE_MANIFEST.json records exact source hashes.

Source: {source}
Patch base: {base}
A contract: {A_CONTRACT}
PR: https://github.com/seradotcom/semwright/pull/172

Prefer continuing the existing effect-conformance worktree and branch. Do not reset it, change another branch, or apply this patch over existing F files. In a separate owned worktree based exactly on the patch base, run git apply --check /path/F_SOURCE.patch before git apply /path/F_SOURCE.patch. Check all restored file hashes against SOURCE_MANIFEST.json. Source-only reconstruction was already verified in a disposable temporary folder; that is not a build, installation, native acceptance or security claim.

All Cargo/build/test, Godot, Blender, fuzz and mutation workloads belong in GitHub-hosted Actions. Use the registered selector in scripts/effects/lane.json and verify the exact tested SHA. Re-running an older workflow does not test newer code. Reports distinguish contractual consumers, native-adapter conformance and production Broker E2E.

Historical docs and CI receipts apply only to their stated source SHA. Packaging establishes no current-SHA acceptance. Owner A review, D/E production adapters, final A/B/C receipts, native negative cases, portability, targeted mutations, audit/license and clean-install evidence must be checked separately. No main merge, release, demo, R16 closure, official trust badge, global noninterference, crash durability or cross-app rollback is authorized by this backup.
"""
    payloads = {"F_SOURCE.patch": patch,
                "SOURCE_MANIFEST.json": (json.dumps(manifest, indent=2, sort_keys=True) + "\n").encode(),
                "RESTORE.md": readme.encode()}
    output = Path(output)
    output.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
        for name, data in sorted(payloads.items()):
            entry = zipfile.ZipInfo(name, (1980, 1, 1, 0, 0, 0))
            entry.create_system = 3
            entry.external_attr = 0o100644 << 16
            entry.compress_type = zipfile.ZIP_DEFLATED
            archive.writestr(entry, data, compress_type=zipfile.ZIP_DEFLATED, compresslevel=9)
    digest = sha(output.read_bytes())
    output.with_suffix(output.suffix + ".sha256").write_text(digest + "  " + output.name + "\n")
    return {"path": str(output), "bytes": output.stat().st_size, "sha256": digest, "files": len(files), "source_sha": source}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", required=True)
    parser.add_argument("--base", default=DEFAULT_BASE)
    parser.add_argument("--output", required=True)
    args = parser.parse_args()
    print(json.dumps(build(args.source, args.base, args.output), indent=2))
