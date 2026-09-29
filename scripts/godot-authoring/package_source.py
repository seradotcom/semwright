#!/usr/bin/env python3
"""Deterministic D source backup from immutable Git objects.

Packaging is source reconstruction only. It does not establish native acceptance,
release readiness, security certification, or R16 closure.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parents[2]
PATCH_BASE = "c14f79f9a59115a56967b53d6abcf8a027d7a132"
A_C0 = "26602e4b25929be869d69ef28fef4dd9713180d7"
C_P0 = "6ee52b428310370d3ad438a13964086a63f48367"
F_SOURCE = "b30d693c24d7dc0527834b7830823655be5216ce"
PR = 176
OWNED = (
    "crates/driver-godot/",
    "integrations/godot/authoring/",
    "docs/godot/authoring/",
    "scripts/godot-authoring/",
    "skills/semwright-godot-production/",
)
SINGLES = (
    ".github/workflows/godot-authoring.yml",
    "Cargo.toml",
    "Cargo.lock",
)

def run_git(*args, check=True):
    result = subprocess.run(
        ["git", "-C", str(ROOT), *args],
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False,
    )
    if check and result.returncode:
        raise RuntimeError(result.stderr.decode("utf-8", "replace")[-4000:])
    return result

def git(*args):
    return run_git(*args).stdout

def sha(data):
    return hashlib.sha256(data).hexdigest()

def tree(commit, path):
    return git("rev-parse", f"{commit}:{path}").decode().strip()

def exists(commit, path):
    return run_git("cat-file", "-e", f"{commit}:{path}", check=False).returncode == 0

def current_files(source):
    names = set()
    for root in OWNED:
        names.update(
            git("ls-tree", "-r", "--name-only", source, "--", root)
            .decode().splitlines()
        )
    for name in SINGLES:
        if exists(source, name):
            names.add(name)
    safe = []
    for name in sorted(names):
        parts = Path(name).parts
        if name.startswith("/") or ".." in parts or ".git" in parts or "target" in parts:
            raise ValueError(f"unsafe package path: {name}")
        if "/backup/" in name or "__pycache__" in parts:
            continue
        safe.append(name)
    return safe

def zip_entry(archive, name, data, executable=False):
    entry = zipfile.ZipInfo(name, (1980, 1, 1, 0, 0, 0))
    entry.create_system = 3
    entry.external_attr = (0o100755 if executable else 0o100644) << 16
    entry.compress_type = zipfile.ZIP_DEFLATED
    archive.writestr(entry, data, compress_type=zipfile.ZIP_DEFLATED, compresslevel=9)

def build(source, output):
    if os.environ.get("GITHUB_ACTIONS") != "true" or os.environ.get("RUNNER_ENVIRONMENT") != "github-hosted":
        raise ValueError("D packaging is restricted to GitHub-hosted Actions")
    for commit in [source, PATCH_BASE, A_C0, C_P0, F_SOURCE]:
        if not re.fullmatch(r"[a-f0-9]{40}", commit):
            raise ValueError("dependency IDs must be full immutable commit SHAs")
        run_git("cat-file", "-e", commit + "^{commit}")
    if git("rev-parse", "HEAD").decode().strip() != source or os.environ.get("GITHUB_SHA") != source:
        raise ValueError("package source must equal exact Actions checkout")
    if tree(source, "crates/semantic-composition") != tree(A_C0, "crates/semantic-composition"):
        raise ValueError("A Composition tree changed")
    if tree(source, "crates/project-graph") != tree(C_P0, "crates/project-graph"):
        raise ValueError("C P0 tree changed")
    if tree(source, "crates/effect-conformance") != tree(F_SOURCE, "crates/effect-conformance"):
        raise ValueError("F consumed source changed")

    names = current_files(source)
    if not names or len(names) > 384:
        raise ValueError("unexpected D source file count")
    files = {}
    total = 0
    for name in names:
        data = git("show", f"{source}:{name}")
        if len(data) > 4_194_304:
            raise ValueError(f"single source file exceeds 4 MiB: {name}")
        total += len(data)
        files[name] = data
    if total > 16_777_216:
        raise ValueError("D source package exceeds 16 MiB source budget")

    pathspec = [*OWNED, *SINGLES]
    patch = git(
        "diff", "--no-ext-diff", "--no-textconv", "--binary", "--full-index",
        PATCH_BASE, source, "--", *pathspec,
    )
    if len(patch) > 16_777_216:
        raise ValueError("D reconstruction patch exceeds budget")

    with tempfile.TemporaryDirectory(prefix="D-source-restore-") as temp:
        restore = Path(temp)
        base_names = set()
        for root in OWNED:
            base_names.update(
                git("ls-tree", "-r", "--name-only", PATCH_BASE, "--", root)
                .decode().splitlines()
            )
        for name in SINGLES:
            if exists(PATCH_BASE, name):
                base_names.add(name)
        for name in sorted(base_names):
            data = git("show", f"{PATCH_BASE}:{name}")
            target = restore / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(data)
        subprocess.run(["git", "init", "--quiet", str(restore)], check=True)
        for dry in [True, False]:
            command = ["git", "-C", str(restore), "-c", "core.autocrlf=false", "apply"]
            if dry:
                command.append("--check")
            command.append("-")
            subprocess.run(command, input=patch, check=True)
        for name, expected in files.items():
            restored = restore / name
            if not restored.is_file() or restored.read_bytes() != expected:
                raise ValueError(f"restored source mismatch: {name}")
        source_set = set(names)
        deleted = base_names - source_set
        for name in deleted:
            if (restore / name).exists():
                raise ValueError(f"deleted source survived reconstruction: {name}")

    manifest = {
        "schema_version": 1,
        "role": "D",
        "source_sha": source,
        "pr": PR,
        "baseline_sha": "b736d41b61c4a4146c9e75c16796e251b025e69f",
        "patch_base_sha": PATCH_BASE,
        "dependencies": {"a_c0": A_C0, "c_p0": C_P0, "f_source": F_SOURCE},
        "files": {
            name: {"sha256": sha(data), "bytes": len(data)}
            for name, data in files.items()
        },
        "source_bytes": total,
        "patch_sha256": sha(patch),
        "reconstruction": "git apply --check + apply + byte-for-byte comparison passed",
        "build_or_native_execution": False,
        "godot_authoring_ready": False,
    }
    restore_md = f"""# Semwright Godot authoring D source backup

Source SHA: {source}
Patch base: {PATCH_BASE}
PR: https://github.com/seradotcom/semwright/pull/{PR}

This deterministic ZIP is a source backup, not a release or acceptance result.
Use a separate owned checkout at exactly PATCH_BASE, verify D_SOURCE.patch with
git apply --check, apply it, then compare selected paths to SOURCE_MANIFEST.json.

A/C/F dependencies are pinned in the manifest and are not duplicated as alternate
authorities. No .git, target/, native runtimes, import caches, export binaries,
credentials, user files or prior backup ZIPs are included. Build, Godot, export,
hostile and final exact-SHA validation remain GitHub-hosted Actions obligations.
"""
    output = Path(output)
    output.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
        zip_entry(archive, "D_SOURCE.patch", patch)
        zip_entry(
            archive, "SOURCE_MANIFEST.json",
            (json.dumps(manifest, indent=2, sort_keys=True) + "\n").encode(),
        )
        zip_entry(archive, "RESTORE.md", restore_md.encode())
        for name, data in sorted(files.items()):
            executable = name.startswith("scripts/") and data.startswith(b"#!")
            zip_entry(archive, "source/" + name, data, executable)
    digest = sha(output.read_bytes())
    output.with_suffix(output.suffix + ".sha256").write_text(
        f"{digest}  {output.name}\n"
    )
    print(json.dumps({
        "source_sha": source, "path": str(output), "bytes": output.stat().st_size,
        "sha256": digest, "files": len(files), "patch_base_sha": PATCH_BASE,
    }, indent=2))

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", required=True)
    parser.add_argument("--output", required=True)
    args = parser.parse_args()
    build(args.source, args.output)
