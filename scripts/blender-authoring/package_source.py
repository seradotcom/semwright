#!/usr/bin/env python3
"""Build a deterministic E source-only backup from immutable Git objects.

Packaging is not a build, native acceptance, release, or security certification.
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
PATCH_BASE = "908b0294a8a7c3eb3aa0877ba7562c178260c89f"
A_C0 = "26602e4b25929be869d69ef28fef4dd9713180d7"
C_P0 = "6ee52b428310370d3ad438a13964086a63f48367"
F_SOURCE = "5ed7d0ff8016d76031ec33846f27a237f196835c"
GLB_SOURCE = "74671c11dda2133ce6af939896c49cdbb6ba47d5"
OWNED = (
    "crates/driver-blender/",
    "adapters/blender/semwright_blender/",
    "fixtures/blender-authoring/",
    "scripts/blender-authoring/",
    "skills/semwright-blender-production/",
    "docs/blender/authoring/",
)
SINGLES = (
    ".ci/blender-authoring.json",
    ".github/workflows/blender-authoring.yml",
    "fuzz/Cargo.toml",
    "fuzz/fuzz_targets/blender_authoring.rs",
    "Cargo.toml",
    "Cargo.lock",
)


def git(*args, check=True):
    proc = subprocess.run(
        ["git", "-C", str(ROOT), *args],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if check and proc.returncode:
        raise RuntimeError(proc.stderr.decode("utf-8", "replace")[-4000:])
    return proc.stdout


def sha(data):
    return hashlib.sha256(data).hexdigest()


def tree(commit, path):
    return git("rev-parse", f"{commit}:{path}").decode().strip()


def source_files(source):
    names = set()
    for root in OWNED:
        names.update(
            git("ls-tree", "-r", "--name-only", source, "--", root)
            .decode()
            .splitlines()
        )
    for name in SINGLES:
        if git("cat-file", "-e", f"{source}:{name}", check=False) == b"":
            # cat-file writes no stdout on success; verify through return-code helper below.
            proc = subprocess.run(
                ["git", "-C", str(ROOT), "cat-file", "-e", f"{source}:{name}"],
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            )
            if proc.returncode == 0:
                names.add(name)
    result = []
    for name in sorted(names):
        parts = Path(name).parts
        if ".." in parts or name.startswith("/") or ".git" in parts or "target" in parts:
            raise ValueError(f"unsafe package path: {name}")
        if name.startswith("docs/blender/authoring/backup/"):
            continue
        result.append(name)
    return result


def zip_entry(archive, name, data, executable=False):
    entry = zipfile.ZipInfo(name, (1980, 1, 1, 0, 0, 0))
    entry.create_system = 3
    entry.external_attr = (0o100755 if executable else 0o100644) << 16
    entry.compress_type = zipfile.ZIP_DEFLATED
    archive.writestr(entry, data, compress_type=zipfile.ZIP_DEFLATED, compresslevel=9)


def build(source, output):
    if os.environ.get("GITHUB_ACTIONS") != "true" or os.environ.get("RUNNER_ENVIRONMENT") != "github-hosted":
        raise ValueError("E source packaging is restricted to GitHub-hosted Actions")
    for commit in [source, PATCH_BASE, A_C0, C_P0, F_SOURCE, GLB_SOURCE]:
        if not re.fullmatch(r"[a-f0-9]{40}", commit):
            raise ValueError("all dependency IDs must be full immutable commit SHAs")
        git("cat-file", "-e", commit + "^{commit}")
    if git("rev-parse", "HEAD").decode().strip() != source or os.environ.get("GITHUB_SHA") != source:
        raise ValueError("package source must equal the exact Actions checkout")
    if tree(source, "crates/semantic-composition") != tree(A_C0, "crates/semantic-composition"):
        raise ValueError("A C0 tree changed; version/review before packaging E")
    if tree(source, "crates/project-graph") != tree(C_P0, "crates/project-graph"):
        raise ValueError("C P0 tree changed; version/review before packaging E")
    if tree(source, "crates/effect-conformance") != tree(F_SOURCE, "crates/effect-conformance"):
        raise ValueError("F consumed source changed; reconcile before packaging E")

    names = source_files(source)
    if not names or len(names) > 256:
        raise ValueError("unexpected E source file count")
    files = {}
    total = 0
    for name in names:
        data = git("show", f"{source}:{name}")
        if len(data) > 2_097_152:
            raise ValueError(f"single source file exceeds 2 MiB: {name}")
        total += len(data)
        files[name] = data
    if total > 8_388_608:
        raise ValueError("E source package exceeds 8 MiB source budget")

    patch = git(
        "diff",
        "--no-ext-diff",
        "--no-textconv",
        "--binary",
        "--full-index",
        PATCH_BASE,
        source,
        "--",
        *names,
    )
    if len(patch) > 8_388_608:
        raise ValueError("E reconstruction patch exceeds budget")

    with tempfile.TemporaryDirectory(prefix="E-source-restore-") as temp:
        restore = Path(temp)
        for name in names:
            proc = subprocess.run(
                ["git", "-C", str(ROOT), "show", f"{PATCH_BASE}:{name}"],
                stdout=subprocess.PIPE,
                stderr=subprocess.DEVNULL,
            )
            if proc.returncode == 0:
                target = restore / name
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(proc.stdout)
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

    manifest = {
        "schema_version": 1,
        "role": "E",
        "source_sha": source,
        "pr": 175,
        "patch_base_sha": PATCH_BASE,
        "dependencies": {
            "a_c0": A_C0,
            "c_p0": C_P0,
            "f_source": F_SOURCE,
            "glb_source": GLB_SOURCE,
        },
        "files": {name: {"sha256": sha(data), "bytes": len(data)} for name, data in files.items()},
        "source_bytes": total,
        "patch_sha256": sha(patch),
        "reconstruction": "git apply --check + apply + byte-for-byte comparison passed",
        "build_or_native_execution": False,
        "blender_authoring_ready": False,
    }
    restore = f"""# Semwright Blender authoring E source backup

Source SHA: {source}
Patch base: {PATCH_BASE}
PR: https://github.com/seradotcom/semwright/pull/175

This ZIP is a deterministic source backup, not a release or acceptance result.
The patch base already contains the frozen A/C/F/GLB dependencies documented in
SOURCE_MANIFEST.json. Prefer the existing E branch/worktree. To reconstruct in a
separate owned checkout at exactly PATCH_BASE, verify E_SOURCE.patch with
`git apply --check` before applying it, then compare every selected path against
SOURCE_MANIFEST.json.

The archive contains no .git directory, target/, native runtime, render frames,
credentials, user files, or prior backup ZIPs. Heavy build, Blender, fuzz, and
native validation still belong in GitHub-hosted Actions on the exact source SHA.
Godot roundtrip remains a separate D-owned public integration gate.
"""
    output = Path(output)
    output.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
        zip_entry(archive, "E_SOURCE.patch", patch)
        zip_entry(
            archive,
            "SOURCE_MANIFEST.json",
            (json.dumps(manifest, indent=2, sort_keys=True) + "\n").encode(),
        )
        zip_entry(archive, "RESTORE.md", restore.encode())
        for name, data in sorted(files.items()):
            executable = name.startswith("scripts/") and data.startswith(b"#!")
            zip_entry(archive, "source/" + name, data, executable)
    digest = sha(output.read_bytes())
    checksum = output.with_suffix(output.suffix + ".sha256")
    checksum.write_text(f"{digest}  {output.name}\n")
    print(json.dumps({
        "source_sha": source,
        "path": str(output),
        "bytes": output.stat().st_size,
        "sha256": digest,
        "files": len(files),
        "patch_base_sha": PATCH_BASE,
    }, indent=2))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", required=True)
    parser.add_argument("--output", required=True)
    args = parser.parse_args()
    build(args.source, args.output)
