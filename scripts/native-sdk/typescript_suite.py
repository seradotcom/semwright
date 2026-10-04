#!/usr/bin/env python3
"""Compile/test the private TypeScript binding and build the pinned inventory bundle.

Dependency installation is deliberately outside this script so Actions/CircleCI can
use the committed lockfile once it exists. This script never publishes a package.
"""
from __future__ import annotations

import hashlib
import json
import os
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SDK = ROOT / "sdk/native-typescript"
OUT = ROOT / "verification/native-sdk/typescript"
COMPILED = OUT / "compiled"
BUNDLE = OUT / "inventory.cjs"
MAX_BUNDLE = 48 * 1024


def run(argv: list[str], *, cwd: Path = ROOT) -> str:
    process = subprocess.run(
        argv,
        cwd=cwd,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        timeout=180,
    )
    print("$", " ".join(argv), flush=True)
    print(process.stdout, end="", flush=True)
    if process.returncode:
        raise SystemExit(process.returncode)
    return process.stdout


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main() -> None:
    if os.getenv("GITHUB_ACTIONS") != "true" and os.getenv("CIRCLECI") != "true":
        raise SystemExit("TypeScript binding suite is CI-only")
    version = run(["node", "--version"]).strip()
    match = re.fullmatch(r"v(\d+)\.(\d+)\.(\d+)", version)
    if not match or int(match.group(1)) != 24 or (int(match.group(2)), int(match.group(3))) < (21, 0):
        raise SystemExit(f"Node >=24.21.0 <25 required, found {version}")

    required = [
        SDK / "node_modules/typescript/bin/tsc",
        SDK / "node_modules/esbuild/bin/esbuild",
    ]
    if any(not path.is_file() for path in required):
        raise SystemExit("Locked TypeScript dependencies are not installed")

    OUT.mkdir(parents=True, exist_ok=True)
    run(["npm", "run", "build"], cwd=SDK)
    run(["npm", "run", "build:tests"], cwd=SDK)
    tap = run(["npm", "test"], cwd=SDK)

    tests = re.search(r"(?m)^# tests (\d+)\s*$", tap)
    passed = re.search(r"(?m)^# pass (\d+)\s*$", tap)
    failed = re.search(r"(?m)^# fail (\d+)\s*$", tap)
    skipped = re.search(r"(?m)^# skipped (\d+)\s*$", tap)
    cancelled = re.search(r"(?m)^# cancelled (\d+)\s*$", tap)
    todo = re.search(r"(?m)^# todo (\d+)\s*$", tap)
    if not tests or not passed or not failed:
        raise SystemExit("Node test summary was not observed")
    count = int(tests.group(1))
    if count <= 0 or int(passed.group(1)) != count or int(failed.group(1)) != 0:
        raise SystemExit("TypeScript suite did not fully pass")
    for label, marker in [("skipped", skipped), ("cancelled", cancelled), ("todo", todo)]:
        if marker and int(marker.group(1)) != 0:
            raise SystemExit(f"TypeScript suite contains {label} tests")

    esbuild = SDK / "node_modules/esbuild/bin/esbuild"
    run([
        str(esbuild),
        str(ROOT / "examples/native-inventory/entry.ts"),
        "--bundle",
        "--platform=node",
        "--target=node24",
        "--format=cjs",
        "--minify",
        "--legal-comments=none",
        f"--outfile={BUNDLE}",
    ])
    size = BUNDLE.stat().st_size
    if size == 0 or size > MAX_BUNDLE:
        raise SystemExit(f"Inventory bundle is outside the bridge budget: {size} bytes")

    lock = SDK / "package-lock.json"
    report = {
        "schema_version": 1,
        "node": version,
        "tests": count,
        "bundle": str(BUNDLE.relative_to(ROOT)),
        "bundle_bytes": size,
        "bundle_sha256": sha256(BUNDLE),
        "sdk_source_sha256": sha256(SDK / "src/index.ts"),
        "inventory_source_sha256": sha256(ROOT / "examples/native-inventory/application.ts"),
        "package_lock_sha256": sha256(lock) if lock.is_file() else None,
        "published": False,
    }
    (OUT / "binding.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    # run-suite.py intentionally consumes this conventional unittest-style line.
    print(f"Ran {count} tests")
    print(f"PINNED_BUNDLE_SHA256={report['bundle_sha256']}")


if __name__ == "__main__":
    main()
