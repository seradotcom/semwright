#!/usr/bin/env python3
"""Select affected Godot authoring CI lanes or force exact-SHA certification."""

from __future__ import annotations

import argparse
import fnmatch
import json
import os
import re
import subprocess
from pathlib import Path

LANES = ("model", "native", "persistence", "export", "hostile", "cross_app")
HOST_FILE = "crates/driver-godot/tests/authoring_host.rs"
WORKFLOW_FILE = ".github/workflows/godot-authoring.yml"
WORKFLOW_JOB_LANES = {
    "scope": frozenset(),
    "godot-model": frozenset(("model",)),
    "godot-native-authoring": frozenset(("native",)),
    "godot-persistence": frozenset(("persistence",)),
    "godot-export": frozenset(("export",)),
    "godot-hostile": frozenset(("hostile",)),
    "godot-cross-app-glb": frozenset(("cross_app",)),
    "godot-source-package": frozenset(),
}
WORKFLOW_CROSS_APP_STEPS = frozenset(
    (
        "Acquire pinned E Blender GLB evidence for D12",
        "D12 Blender replacement through Broker and Driver Host",
    )
)
HOST_EXTRA_LANES = frozenset(("native", "persistence", "export", "cross_app"))
NATIVE_SCENARIOS = (
    "driver_host_handshake_control_reaches_capabilities",
    "empty_project_authoring_flows_through_broker_driver_host_and_provider",
    "animation_tree_state_machine_and_blend_space_round_trip_natively",
    "shared_and_local_to_scene_materials_are_native_and_isolated",
    "typed_transform_and_reparent_actions_round_trip_natively",
)
HOST_TEST_LANES = {
    "persistence_lane_reopens_in_fresh_process_and_preserves_dependencies": frozenset(("persistence",)),
    "export_lane_builds_and_launches_without_editor_or_semwright": frozenset(("export",)),
    "blender_glb_handoff_preserves_godot_semantics_and_gameplay": frozenset(("cross_app",)),
}
HOST_NATIVE_ONLY_TESTS = frozenset(NATIVE_SCENARIOS)
RUST_FN = re.compile(r"^(?:async\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(")
HUNK = re.compile(r"^@@\s+-\d+(?:,\d+)?\s+\+(\d+)(?:,(\d+))?\s+@@")
YAML_JOB = re.compile(r"^  ([A-Za-z0-9_-]+):\s*$")
YAML_STEP = re.compile(r'^      - name:\s+["\']?(.+?)["\']?\s*$')

ALL_PATTERNS = (
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
        "crates/driver-godot/tests/fixtures/authoring/three_d.json",
        "crates/driver-godot/tests/fixtures/authoring/triangle.glb",
        "crates/platform-common/src/artifact.rs",
    ),
}


def matches(path: str, patterns: tuple[str, ...]) -> bool:
    return any(fnmatch.fnmatchcase(path, pattern) for pattern in patterns)


def rust_function_spans(source: str) -> list[tuple[int, int, str]]:
    starts: list[tuple[int, str]] = []
    for line_number, line in enumerate(source.splitlines(), 1):
        match = RUST_FN.match(line)
        if match:
            starts.append((line_number, match.group(1)))
    spans: list[tuple[int, int, str]] = []
    last_line = max(1, len(source.splitlines()))
    for index, (start, name) in enumerate(starts):
        end = starts[index + 1][0] - 1 if index + 1 < len(starts) else last_line
        spans.append((start, end, name))
    return spans


def owner_for_line(spans: list[tuple[int, int, str]], line: int) -> str | None:
    for start, end, name in spans:
        if start <= line <= end:
            return name
    return None


def host_lanes_for_changed_lines(source: str, changed_lines: set[int]) -> set[str]:
    if not changed_lines:
        return set(HOST_EXTRA_LANES)
    spans = rust_function_spans(source)
    lanes: set[str] = set()
    for line in changed_lines:
        owner = owner_for_line(spans, line)
        if owner in HOST_TEST_LANES:
            lanes.update(HOST_TEST_LANES[owner])
        elif owner in HOST_NATIVE_ONLY_TESTS:
            lanes.add("native")
        else:
            # Helpers and unknown/deleted regions can affect every host-backed lane.
            return set(HOST_EXTRA_LANES)
    return lanes


def host_scenarios_for_changed_lines(source: str, changed_lines: set[int]) -> set[str]:
    if not changed_lines:
        return set(NATIVE_SCENARIOS)
    spans = rust_function_spans(source)
    scenarios: set[str] = set()
    for line in changed_lines:
        owner = owner_for_line(spans, line)
        if owner in HOST_NATIVE_ONLY_TESTS:
            scenarios.add(owner)
        elif owner == "blender_glb_handoff_preserves_godot_semantics_and_gameplay":
            scenarios.add("driver_host_handshake_control_reaches_capabilities")
        elif owner in HOST_TEST_LANES:
            continue
        else:
            return set(NATIVE_SCENARIOS)
    return scenarios


def yaml_named_spans(source: str, pattern: re.Pattern[str]) -> list[tuple[int, int, str]]:
    starts: list[tuple[int, str]] = []
    for line_number, line in enumerate(source.splitlines(), 1):
        match = pattern.match(line)
        if match:
            starts.append((line_number, match.group(1)))
    spans: list[tuple[int, int, str]] = []
    last_line = max(1, len(source.splitlines()))
    for index, (start, name) in enumerate(starts):
        end = starts[index + 1][0] - 1 if index + 1 < len(starts) else last_line
        spans.append((start, end, name))
    return spans


def workflow_lanes_for_changed_lines(source: str, changed_lines: set[int]) -> set[str]:
    if not changed_lines:
        return set(LANES)
    job_spans = yaml_named_spans(source, YAML_JOB)
    step_spans = yaml_named_spans(source, YAML_STEP)
    lanes: set[str] = set()
    for line in changed_lines:
        job = owner_for_line(job_spans, line)
        if job is None:
            # Trigger/concurrency/default changes are scheduler-only; scope itself
            # remains the executable validation for those edits.
            continue
        if job not in WORKFLOW_JOB_LANES:
            return set(LANES)
        lanes.update(WORKFLOW_JOB_LANES[job])
        if job == "godot-native-authoring":
            step = owner_for_line(step_spans, line)
            if step in WORKFLOW_CROSS_APP_STEPS:
                lanes.add("cross_app")
    return lanes


def changed_new_lines(base: str, head: str, path: str) -> set[int]:
    if not base or set(base) == {"0"}:
        return set()
    try:
        subprocess.check_call(
            ["git", "cat-file", "-e", f"{base}^{{commit}}"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        output = git("diff", "--unified=0", base, head, "--", path)
    except subprocess.CalledProcessError:
        return set()
    lines: set[int] = set()
    for line in output.splitlines():
        match = HUNK.match(line)
        if not match:
            continue
        start = int(match.group(1))
        count = int(match.group(2) or "1")
        if count == 0:
            lines.add(max(1, start))
        else:
            lines.update(range(start, start + count))
    return lines


def host_changed_lanes(base: str, head: str) -> set[str]:
    try:
        source = git("show", f"{head}:{HOST_FILE}")
    except subprocess.CalledProcessError:
        return set(HOST_EXTRA_LANES)
    return host_lanes_for_changed_lines(source, changed_new_lines(base, head, HOST_FILE))


def host_changed_scenarios(base: str, head: str) -> set[str]:
    try:
        source = git("show", f"{head}:{HOST_FILE}")
    except subprocess.CalledProcessError:
        return set(NATIVE_SCENARIOS)
    return host_scenarios_for_changed_lines(
        source, changed_new_lines(base, head, HOST_FILE)
    )


def workflow_changed_lanes(base: str, head: str) -> set[str]:
    try:
        source = git("show", f"{head}:{WORKFLOW_FILE}")
    except subprocess.CalledProcessError:
        return set(LANES)
    return workflow_lanes_for_changed_lines(
        source, changed_new_lines(base, head, WORKFLOW_FILE)
    )


def select(
    paths: list[str],
    certify: bool,
    host_extra_lanes: set[str] | None = None,
    workflow_extra_lanes: set[str] | None = None,
) -> dict[str, bool]:
    if certify:
        return {lane: True for lane in LANES}

    selected = {lane: False for lane in LANES}
    for path in paths:
        if matches(path, ALL_PATTERNS):
            return {lane: True for lane in LANES}
        for lane, patterns in RULES.items():
            if matches(path, patterns):
                selected[lane] = True

    if HOST_FILE in paths:
        for lane in host_extra_lanes or ():
            selected[lane] = True
    if WORKFLOW_FILE in paths:
        for lane in workflow_extra_lanes or ():
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

    host_extra = (
        host_changed_lanes(args.base, args.head)
        if not certify and HOST_FILE in paths
        else set()
    )
    workflow_extra = (
        workflow_changed_lanes(args.base, args.head)
        if not certify and WORKFLOW_FILE in paths
        else set()
    )
    lanes = select(paths, certify, host_extra, workflow_extra)
    if certify:
        native_scenarios = set(NATIVE_SCENARIOS)
    elif lanes["native"]:
        non_host_native = any(
            path != HOST_FILE
            and (
                matches(path, ALL_PATTERNS)
                or matches(path, RULES["native"])
                or path == WORKFLOW_FILE
            )
            for path in paths
        )
        native_scenarios = (
            set(NATIVE_SCENARIOS)
            if non_host_native
            else host_changed_scenarios(args.base, args.head)
        )
        if not native_scenarios:
            native_scenarios = set(NATIVE_SCENARIOS)
    else:
        native_scenarios = set()
    report = {
        "schema_version": 1,
        "event": args.event,
        "head_sha": args.head,
        "base_sha": args.base or None,
        "certify": certify,
        "certification_trigger": trigger,
        "changed_paths": paths,
        "host_extra_lanes": sorted(host_extra),
        "workflow_extra_lanes": sorted(workflow_extra),
        "native_scenarios": [
            scenario for scenario in NATIVE_SCENARIOS if scenario in native_scenarios
        ],
        "lanes": lanes,
    }
    report_path = Path(args.report)
    report_path.parent.mkdir(parents=True, exist_ok=True)
    report_path.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")

    values = {
        "certify": str(certify).lower(),
        "native_scenarios": ",".join(
            scenario for scenario in NATIVE_SCENARIOS if scenario in native_scenarios
        ),
        **{lane: str(enabled).lower() for lane, enabled in lanes.items()},
    }
    write_outputs(args.github_output, values)
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
