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
COMPOSITION_CONTRACT = "26602e4b25929be869d69ef28fef4dd9713180d7"
DEFAULT_BASE = "a6b5bebbb022ae1e29c2b8e1ec3d4358cee7fb35"
DEPENDENCY_VERSION = 4
COMPOSITION_COMPONENT_SOURCE = "6e1261d645699e99fe94ad902b5fb26956f92102"
HOST_COMPONENT_SOURCE = "a6b5bebbb022ae1e29c2b8e1ec3d4358cee7fb35"


def git(*args):
    return subprocess.check_output(["git", "-C", str(ROOT), *args])


def sha(data):
    return hashlib.sha256(data).hexdigest()


def build(source, base, output):
    for value in (source, base):
        if not re.fullmatch(r"[a-f0-9]{40}", value):
            raise ValueError("source and base must be full immutable SHA-1 commit IDs")
        git("cat-file", "-e", value + "^{commit}")
    if git("rev-parse", source + ":crates/semantic-composition") != git("rev-parse", COMPOSITION_COMPONENT_SOURCE + ":crates/semantic-composition"):
        raise ValueError("Composition shared contract changed; review and version the backup dependency first")
    if git("rev-parse", source + ":crates/project-graph") != git("rev-parse", base + ":crates/project-graph"):
        raise ValueError("Project Graph P0 changed; review and version the backup dependency first")
    for path in ["crates/driver-sdk", "crates/driver-host"]:
        if git("rev-parse", source + ":" + path) != git("rev-parse", HOST_COMPONENT_SOURCE + ":" + path):
            raise ValueError("Reviewed Host transport changed; review and version the backup dependency first")
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
    selected = [*files, "Cargo.toml", "Cargo.lock"]
    patch = git("diff", "--no-ext-diff", "--no-textconv", "--binary", "--full-index", base, source, "--", *selected)
    lock = git("show", source + ":Cargo.lock")
    workspace = git("show", source + ":Cargo.toml")
    with tempfile.TemporaryDirectory(prefix="effect-conformance-patch-reconstruction-") as temp:
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
        for name, expected in {**files, "Cargo.toml": workspace, "Cargo.lock": lock}.items():
            if (root / name).read_bytes() != expected:
                raise ValueError("restored source mismatch: " + name)
    manifest = {"schema_version": 1, "role": "effect-conformance", "source_sha": source,
                "patch_base_sha": base, "contract_sha": COMPOSITION_CONTRACT,
                "dependency_version": DEPENDENCY_VERSION,
                "composition_component_source": COMPOSITION_COMPONENT_SOURCE,
                "host_component_source": HOST_COMPONENT_SOURCE,
                "main_baseline_sha": "b736d41b61c4a4146c9e75c16796e251b025e69f",
                "files": {name: sha(data) for name, data in files.items()},
                "lock_sha256": sha(lock), "workspace_sha256": sha(workspace), "patch_sha256": sha(patch),
                "reconstruction": "git apply --check and byte-for-byte source comparison passed",
                "build_or_native_execution": False,
                "acceptance": "NOT_ESTABLISHED_BY_PACKAGING"}
    readme = f"""# Effect Conformance source backup — not an accepted release

This is a reconstructive patch ZIP, not a full repository checkout. EFFECT_CONFORMANCE_SOURCE.patch contains all {len(files)} Effect Conformance implementation, contract, test, native harness, CI and documentation files plus the Cargo.toml/Cargo.lock deltas. SOURCE_MANIFEST.json records exact source hashes.

Source: {source}
Patch base: {base}
Historical Composition contract: {COMPOSITION_CONTRACT}
Reviewed Composition implementation: {COMPOSITION_COMPONENT_SOURCE}
Reviewed Host transport: {HOST_COMPONENT_SOURCE}
Dependency version: {DEPENDENCY_VERSION}
PR: https://github.com/seradotcom/semwright/pull/172

To reconstruct the source package, start from a clean checkout at exactly the recorded patch base. Run `git apply --check /path/EFFECT_CONFORMANCE_SOURCE.patch` before applying the patch, then verify restored file hashes against `SOURCE_MANIFEST.json`. Source-only reconstruction establishes package integrity only; it is not a build, installation, native acceptance or security claim.

All Cargo/build/test, Godot, Blender, fuzz and mutation workloads belong in GitHub-hosted Actions. Use the registered selector in scripts/effects/lane.json and verify the exact tested SHA. Re-running an older workflow does not test newer code. Reports distinguish contractual consumers, native-adapter conformance and production Broker E2E.

Historical docs and CI receipts apply only to their stated source SHA. Packaging establishes no current-SHA acceptance. Composition review, Godot/Blender production adapters, final Composition/Audio/Project Graph receipts, native negative cases, portability, targeted mutations, audit/license and clean-install evidence must be checked separately. No main merge, release, demo, R16 closure, official trust badge, global noninterference, crash durability or cross-app rollback is authorized by this backup.
"""
    payloads = {"EFFECT_CONFORMANCE_SOURCE.patch": patch,
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
