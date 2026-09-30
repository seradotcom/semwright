#!/usr/bin/env python3
"""Classify a Blender-authoring push into narrow iteration areas or full certification."""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import subprocess

CONFIG_PATH = Path(".ci/blender-authoring.json")

MODEL_PREFIXES = (
    "crates/driver-blender/src/authoring/",
    "crates/driver-blender/tests/authoring_model.rs",
    "fixtures/blender-authoring/",
)
NATIVE_PREFIXES = (
    "crates/driver-blender/src/authoring/",
    "crates/driver-blender/src/authoring_native.py",
    "crates/driver-blender/src/authoring_runtime.rs",
    "crates/driver-blender/src/bridge.py",
    "crates/driver-blender/src/main.rs",
    "crates/driver-blender/tests/authoring_native.rs",
    "fixtures/blender-authoring/",
)
EXPORT_PREFIXES = (
    "adapters/blender/semwright_blender/commands.py",
    "adapters/blender/semwright_blender/export_scope.py",
    "crates/driver-blender/tests/authoring_hostile.py",
    "crates/driver-blender/tests/glb_roundtrip_oracle.py",
)
SECURITY_PREFIXES = (
    "crates/driver-blender/src/main.rs",
    "crates/driver-blender/src/authoring_runtime.rs",
    "crates/driver-blender/driver.manifest.example.json",
    "scripts/dev/blender-driver-smoke.sh",
)
FUZZ_PREFIXES = (
    "fuzz/fuzz_targets/blender_authoring.rs",
    "crates/driver-blender/src/authoring/model.rs",
    "crates/driver-blender/src/authoring/plan.rs",
)
SKILL_PREFIXES = ("skills/semwright-blender-production/",)
PACKAGE_PREFIXES = (
    "scripts/blender-authoring/package_source.py",
    "scripts/blender-authoring/finalize_evidence.py",
)
RUST_BUILD_INPUTS = {
    "Cargo.toml",
    "Cargo.lock",
    "crates/driver-blender/Cargo.toml",
}


def git(*args: str) -> str:
    return subprocess.check_output(["git", *args], text=True).strip()


def matches(path: str, prefixes: tuple[str, ...]) -> bool:
    return any(path == prefix or path.startswith(prefix) for prefix in prefixes)


def changed_paths(base: str, head: str) -> list[str]:
    if not base or set(base) == {"0"}:
        base = f"{head}^"
    try:
        text = git("diff", "--name-only", base, head)
    except subprocess.CalledProcessError:
        text = git("diff", "--name-only", f"{head}^", head)
    return sorted({line for line in text.splitlines() if line})


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--base", default="")
    parser.add_argument("--head", required=True)
    parser.add_argument("--output", required=True)
    args = parser.parse_args()

    head = git("rev-parse", args.head)
    if head != args.head:
        raise SystemExit("head must be an exact SHA")
    config = json.loads(CONFIG_PATH.read_text())
    if set(config) != {"version", "default_mode", "certification_trailer", "certification_request"}:
        raise SystemExit("invalid Blender authoring CI config shape")
    if config["version"] != 2 or config["default_mode"] != "iteration":
        raise SystemExit("unsupported Blender authoring CI config")
    cert_trailer = config["certification_trailer"]
    if not isinstance(cert_trailer, str) or not cert_trailer:
        raise SystemExit("certification trailer must be nonempty")
    message = git("show", "-s", "--format=%B", head)
    certification = cert_trailer in {line.strip() for line in message.splitlines()}
    request = config["certification_request"]
    if request is not None and (not isinstance(request, str) or len(request) != 40):
        raise SystemExit("certification_request must be null or a full parent SHA")
    if certification:
        parent = git("rev-parse", f"{head}^")
        if request != parent:
            raise SystemExit("certification_request must equal the candidate commit parent SHA")
    paths = changed_paths(args.base, head)

    areas = {
        "model": any(matches(path, MODEL_PREFIXES) for path in paths),
        "native": any(matches(path, NATIVE_PREFIXES) for path in paths),
        "export": any(matches(path, EXPORT_PREFIXES) for path in paths),
        "security": any(matches(path, SECURITY_PREFIXES) for path in paths),
        "fuzz": any(matches(path, FUZZ_PREFIXES) for path in paths),
        "skill": any(matches(path, SKILL_PREFIXES) for path in paths),
        "package": any(matches(path, PACKAGE_PREFIXES) for path in paths),
    }

    if any(path in RUST_BUILD_INPUTS for path in paths):
        areas["model"] = True
        areas["native"] = True
    if areas["export"]:
        # Export iterations need a real authored GLB to feed the oracle/hostile gates.
        areas["native"] = True
    if areas["native"]:
        # Native contract changes are always checked against the portable model first.
        areas["model"] = True

    if certification:
        for key in areas:
            areas[key] = True

    need_rust = certification or any(
        areas[key] for key in ("model", "native", "security", "fuzz", "skill")
    )
    need_blender = certification or areas["native"] or areas["export"]

    mode = "certification" if certification else "iteration"
    report = {
        "schema_version": 1,
        "mode": mode,
        "source_sha": head,
        "base_sha": args.base or None,
        "certification_trailer": cert_trailer if certification else None,
        "certification_request": request,
        "changed_paths": paths,
        "areas": areas,
        "need_rust": need_rust,
        "need_blender": need_blender,
    }

    out = Path(args.output)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")

    github_output = os.environ.get("GITHUB_OUTPUT")
    if github_output:
        with open(github_output, "a", encoding="utf-8") as stream:
            stream.write(f"mode={mode}\n")
            stream.write(f"certification={'true' if certification else 'false'}\n")
            stream.write(f"need_rust={'true' if need_rust else 'false'}\n")
            stream.write(f"need_blender={'true' if need_blender else 'false'}\n")
            artifact_name = (
                f"blender-authoring-{head}-blender-native-authoring"
                if certification
                else f"blender-authoring-iteration-{head}"
            )
            stream.write(f"artifact_name={artifact_name}\n")
            for key, value in areas.items():
                stream.write(f"{key}={'true' if value else 'false'}\n")

    print(json.dumps(report, sort_keys=True))


if __name__ == "__main__":
    main()
