#!/usr/bin/env python3
"""Select affected Godot authoring CI lanes or force exact-SHA certification."""

from __future__ import annotations

import argparse
import fnmatch
import json
import os
import subprocess
from pathlib import Path

LANES = ("model", "native", "persistence", "export", "hostile", "cross_app")

ALL_PATTERNS = (
    ".github/workflows/godot-authoring.yml",
    "Cargo.toml",
    "Cargo.lock",
    "crates/driver-godot/Cargo.toml",
    "crates/driver-sdk/**",
    "crates/driver-host/**",
    "crates/plugin-host/**",
    "crates/policy/**",
    "crates/core/**",
    "crates/semantic-composition/**",
    "crates/project-graph/**",
    "crates/effect-conformance/**",
    "crates/platform-common/**",
    "crates/platform-linux-sys/**",
)

RULES = {
    "model": (
        "crates/driver-godot/src/authoring/**",
        "crates/driver-godot/src/catalog.rs",
        "crates/driver-godot/src/lib.rs",
        "crates/driver-godot/tests/authoring.rs",
        "crates/driver-godot/tests/authoring_profile.rs",
        "crates/driver-godot/tests/authoring_store.rs",
        "crates/driver-godot/tests/contracts.rs",
        "crates/driver-godot/tests/fixtures/authoring/**",
        "scripts/godot-authoring/ci.py",
        "scripts/godot-authoring/collector.py",
        "scripts/godot-authoring/test_collector.py",
    ),
    "native": (
        "crates/driver-godot/src/**",
        "crates/driver-godot/tests/authoring_host.rs",
        "crates/driver-godot/tests/authoring_native.rs",
        "crates/driver-godot/tests/contracts.rs",
        "crates/driver-godot/tests/fixtures/authoring/**",
        "integrations/godot/addons/semwright/**",
        "integrations/godot/authoring/**",
        "integrations/godot/tests/readback_contract.gd",
        "integrations/godot/tests/scene_save_contract.gd",
        "scripts/dev/ci-driver-bwrap-profile.sh",
    ),
    "persistence": (
        "crates/driver-godot/src/authoring/io.rs",
        "crates/driver-godot/src/authoring/store.rs",
        "crates/driver-godot/src/authoring/native_observation.rs",
        "crates/driver-godot/src/authoring/runtime.rs",
        "crates/driver-godot/src/config.rs",
        "crates/driver-godot/src/runner.rs",
        "crates/driver-godot/tests/authoring_host.rs",
        "crates/driver-godot/tests/authoring_native.rs",
        "crates/driver-godot/tests/authoring_store.rs",
        "integrations/godot/addons/semwright/**",
        "integrations/godot/authoring/**",
        "integrations/godot/tests/scene_save_contract.gd",
        "scripts/dev/ci-driver-bwrap-profile.sh",
    ),
    "export": (
        "crates/driver-godot/src/authoring/compiler/**",
        "crates/driver-godot/src/authoring/model.rs",
        "crates/driver-godot/src/authoring/runtime.rs",
        "crates/driver-godot/src/authoring/store.rs",
        "crates/driver-godot/src/config.rs",
        "crates/driver-godot/src/runner.rs",
        "crates/driver-godot/tests/authoring_host.rs",
        "crates/driver-godot/tests/fixtures/authoring/**",
        "scripts/dev/ci-driver-bwrap-profile.sh",
    ),
    "hostile": (
        "crates/driver-godot/src/**",
        "crates/driver-godot/tests/authoring.rs",
        "crates/driver-godot/tests/authoring_fuzz.rs",
        "crates/driver-godot/tests/authoring_native.rs",
        "crates/driver-godot/tests/authoring_store.rs",
        "crates/driver-godot/tests/contracts.rs",
        "integrations/godot/authoring/**",
    ),
    "cross_app": (
        "crates/driver-godot/src/authoring/compiler/scene.rs",
        "crates/driver-godot/src/authoring/io.rs",
        "crates/driver-godot/src/authoring/model.rs",
        "crates/driver-godot/src/authoring/native_observation.rs",
        "crates/driver-godot/src/authoring/store.rs",
        "crates/driver-godot/tests/authoring_host.rs",
        "crates/driver-godot/tests/fixtures/authoring/three_d.json",
        "crates/driver-godot/tests/fixtures/authoring/triangle.glb",
        "crates/platform-common/src/artifact.rs",
    ),
}


def matches(path: str, patterns: tuple[str, ...]) -> bool:
    return any(fnmatch.fnmatchcase(path, pattern) for pattern in patterns)


def select(paths: list[str], certify: bool) -> dict[str, bool]:
    if certify:
        return {lane: True for lane in LANES}

    selected = {lane: False for lane in LANES}
    for path in paths:
        if matches(path, ALL_PATTERNS):
            return {lane: True for lane in LANES}
        for lane, patterns in RULES.items():
            if matches(path, patterns):
                selected[lane] = True

    # Cross-app always consumes native evidence from the same SHA.
    if selected["cross_app"]:
        selected["native"] = True
    return selected


def git(*args: str) -> str:
    return subprocess.check_output(["git", *args], text=True).strip()


def changed_paths(base: str, head: str) -> list[str]:
    zero = not base or set(base) == {"0"}
    if zero:
        output = git("show", "--pretty=", "--name-only", head)
    else:
        try:
            subprocess.check_call(
                ["git", "cat-file", "-e", f"{base}^{{commit}}"],
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            )
            output = git("diff", "--name-only", base, head)
        except subprocess.CalledProcessError:
            output = git("show", "--pretty=", "--name-only", head)
    return sorted({line for line in output.splitlines() if line})


def write_outputs(path: str, values: dict[str, str]) -> None:
    with open(path, "a", encoding="utf-8") as handle:
        for key, value in values.items():
            handle.write(f"{key}={value}\n")


def certification_mode(
    event: str, certify_requested: bool, expected_sha: str, head_sha: str
) -> tuple[bool, str | None]:
    if event == "workflow_dispatch":
        if not expected_sha or expected_sha != head_sha:
            raise ValueError(
                "certification expected_sha must equal dispatch SHA: "
                f"expected={expected_sha!r} head={head_sha!r}"
            )
        return True, "workflow_dispatch"
    if certify_requested:
        return True, "commit_marker"
    return False, None


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--event", required=True, choices=("push", "workflow_dispatch"))
    parser.add_argument("--head", required=True)
    parser.add_argument("--base", default="")
    parser.add_argument("--expected-sha", default="")
    parser.add_argument("--certify", choices=("true", "false"), default="false")
    parser.add_argument("--github-output", required=True)
    parser.add_argument("--report", required=True)
    args = parser.parse_args()

    certify_requested = args.certify == "true"
    try:
        certify, trigger = certification_mode(
            args.event, certify_requested, args.expected_sha, args.head
        )
    except ValueError as error:
        raise SystemExit(str(error)) from error

    if certify:
        paths: list[str] = []
    else:
        paths = changed_paths(args.base, args.head)

    lanes = select(paths, certify)
    report = {
        "schema_version": 1,
        "event": args.event,
        "head_sha": args.head,
        "base_sha": args.base or None,
        "certify": certify,
        "certification_trigger": trigger,
        "changed_paths": paths,
        "lanes": lanes,
    }
    report_path = Path(args.report)
    report_path.parent.mkdir(parents=True, exist_ok=True)
    report_path.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")

    values = {
        "certify": str(certify).lower(),
        **{lane: str(enabled).lower() for lane, enabled in lanes.items()},
    }
    write_outputs(args.github_output, values)
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
