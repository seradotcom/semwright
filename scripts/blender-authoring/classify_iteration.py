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
    # Any first-party Blender driver source can change startup, bridge semantics,
    # descriptors or native readback used by E. Keep this broad rather than
    # silently skipping shared driver code.
    "crates/driver-blender/src/",
    "crates/driver-blender/tests/authoring_native.rs",
    "adapters/blender/semwright_blender/",
    "fixtures/blender-authoring/",
    "schemas/commands.json",
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
    "scripts/dev/ci-driver-bwrap-profile.sh",
)
FUZZ_PREFIXES = (
    "fuzz/fuzz_targets/blender_authoring.rs",
    "crates/driver-blender/src/authoring/model.rs",
    "crates/driver-blender/src/authoring/plan.rs",
)
SKILL_PREFIXES = (
    "skills/semwright-blender-production/",
    "scripts/blender-authoring/verify_skill_bundle.sh",
)
PACKAGE_PREFIXES = (
    "scripts/blender-authoring/package_source.py",
    "scripts/blender-authoring/finalize_evidence.py",
)
RUST_BUILD_INPUTS = {
    "Cargo.toml",
    "Cargo.lock",
    "crates/driver-blender/Cargo.toml",
}
SUITE_HARNESS_INPUTS = {
    "scripts/blender-authoring/run-suite.py",
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


def classify_areas(paths: list[str], certification: bool) -> dict[str, bool]:
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
    if any(path in SUITE_HARNESS_INPUTS for path in paths):
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
    return areas


def self_test() -> None:
    def expect(paths: list[str], true_keys: set[str]) -> None:
        areas = classify_areas(paths, False)
        assert {key for key, value in areas.items() if value} == true_keys, (paths, areas)

    expect(["docs/blender/authoring/INTEGRATION.md"], set())
    expect(
        ["adapters/blender/semwright_blender/validation.py"],
        {"model", "native"},
    )
    expect(
        ["adapters/blender/semwright_blender/export_scope.py"],
        {"model", "native", "export"},
    )
    expect(
        ["crates/driver-blender/src/semantic.py"],
        {"model", "native"},
    )
    expect(
        ["schemas/commands.json"],
        {"model", "native"},
    )
    expect(
        ["scripts/blender-authoring/run-suite.py"],
        {"model", "native"},
    )
    expect(
        ["scripts/blender-authoring/verify_skill_bundle.sh"],
        {"skill"},
    )
    expect(
        ["scripts/blender-authoring/package_source.py"],
        {"package"},
    )
    expect(
        ["scripts/dev/ci-driver-bwrap-profile.sh"],
        {"security"},
    )
    certified = classify_areas(["docs/blender/authoring/INTEGRATION.md"], True)
    assert certified and all(certified.values()), certified
    head = "a" * 40
    hosted = {"GITHUB_SHA": head, "GITHUB_EVENT_NAME": "workflow_dispatch",
              "GITHUB_ACTIONS": "true", "RUNNER_ENVIRONMENT": "github-hosted"}
    assert dispatch_certification(head, head, hosted)
    assert not dispatch_certification("", head, {})
    for key, value in [("GITHUB_SHA", "b" * 40), ("GITHUB_EVENT_NAME", "push"),
                       ("GITHUB_ACTIONS", "false"), ("RUNNER_ENVIRONMENT", "self-hosted")]:
        try:
            dispatch_certification(head, head, {**hosted, key: value})
        except SystemExit:
            pass
        else:
            raise AssertionError(f"foreign dispatch context accepted: {key}")
    print("classifier-self-test: ok")


def dispatch_certification(requested: str, head: str, environment: dict) -> bool:
    if not requested:
        return False
    if not (
        requested == head == environment.get("GITHUB_SHA")
        and environment.get("GITHUB_EVENT_NAME") == "workflow_dispatch"
        and environment.get("GITHUB_ACTIONS") == "true"
        and environment.get("RUNNER_ENVIRONMENT") == "github-hosted"
    ):
        raise SystemExit("manual certification requires an exact-SHA GitHub-hosted dispatch")
    return True


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--base", default="")
    parser.add_argument("--head")
    parser.add_argument("--certify-sha", default="")
    parser.add_argument("--output")
    args = parser.parse_args()

    if args.self_test:
        self_test()
        return
    if not args.head or not args.output:
        parser.error("--head and --output are required unless --self-test is used")

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
    manual_certification = dispatch_certification(args.certify_sha, head, dict(os.environ))
    certification = cert_trailer in {line.strip() for line in message.splitlines()} or manual_certification
    request = config["certification_request"]
    if request is not None and (not isinstance(request, str) or len(request) != 40):
        raise SystemExit("certification_request must be null or a full parent SHA")
    if certification:
        parent = git("rev-parse", f"{head}^")
        if request != parent:
            raise SystemExit("certification_request must equal the candidate commit parent SHA")
    paths = changed_paths(args.base, head)
    areas = classify_areas(paths, certification)

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
