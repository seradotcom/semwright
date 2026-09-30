#!/usr/bin/env python3
"""Portable self-tests for affected-lane selection."""

from select_ci_scope import (
    HOST_EXTRA_LANES,
    LANES,
    certification_mode,
    host_lanes_for_changed_lines,
    select,
    workflow_lanes_for_changed_lines,
)


def expect(paths, expected):
    actual = select(paths, False)
    assert actual == {lane: lane in expected for lane in LANES}, (paths, actual, expected)


def main():
    expect(
        ["crates/driver-godot/src/authoring/validate.rs"],
        {"model", "native", "hostile"},
    )
    expect(
        ["crates/driver-godot/src/authoring/store.rs"],
        {"model", "native", "persistence", "export", "hostile", "cross_app"},
    )
    expect(
        ["integrations/godot/tests/scene_save_contract.gd"],
        {"native", "persistence"},
    )
    expect(
        ["crates/driver-godot/src/runner.rs"],
        {"native", "persistence", "export", "hostile"},
    )
    expect(
        ["crates/driver-godot/tests/fixtures/authoring/three_d.json"],
        {"model", "native", "export", "cross_app"},
    )
    expect(
        ["scripts/dev/ci-driver-bwrap-profile.sh"],
        {"native", "persistence", "export"},
    )
    expect(
        ["docs/godot/authoring/INTEGRATION.md"],
        set(),
    )
    expect(
        [".github/workflows/godot-authoring.yml"],
        set(),
    )
    expect(
        ["crates/driver-host/src/lib.rs"],
        set(LANES),
    )

    host_source = """fn shared_helper() {}
async fn driver_host_handshake_control_reaches_capabilities() {}
async fn typed_transform_and_reparent_actions_round_trip_natively() {}
async fn persistence_lane_reopens_in_fresh_process_and_preserves_dependencies() {}
async fn export_lane_builds_and_launches_without_editor_or_semwright() {}
async fn blender_glb_handoff_preserves_godot_semantics_and_gameplay() {}
"""
    assert host_lanes_for_changed_lines(host_source, {3}) == set()
    assert host_lanes_for_changed_lines(host_source, {4}) == {"persistence"}
    assert host_lanes_for_changed_lines(host_source, {5}) == {"export"}
    assert host_lanes_for_changed_lines(host_source, {6}) == {"cross_app"}
    assert host_lanes_for_changed_lines(host_source, {1}) == set(HOST_EXTRA_LANES)
    assert host_lanes_for_changed_lines(host_source, set()) == set(HOST_EXTRA_LANES)

    workflow_source = """name: test
concurrency:
  group: test
jobs:
  scope:
    steps:
      - name: Select scope
        run: echo scope
  godot-native-authoring:
    steps:
      - name: Native smoke
        run: echo native
      - name: D12 Blender replacement through Broker and Driver Host
        run: echo d12
  godot-persistence:
    steps:
      - name: Persistence
        run: echo persistence
"""
    def line_of(source, needle):
        return next(
            line
            for line, text in enumerate(source.splitlines(), 1)
            if needle in text
        )

    assert workflow_lanes_for_changed_lines(
        workflow_source, {line_of(workflow_source, "group: test")}
    ) == set()
    assert workflow_lanes_for_changed_lines(
        workflow_source, {line_of(workflow_source, "run: echo native")}
    ) == {"native"}
    assert workflow_lanes_for_changed_lines(
        workflow_source, {line_of(workflow_source, "run: echo d12")}
    ) == {"native", "cross_app"}
    assert workflow_lanes_for_changed_lines(
        workflow_source, {line_of(workflow_source, "run: echo persistence")}
    ) == {"persistence"}

    certified = select([], True)
    assert all(certified.values()), certified

    assert certification_mode("push", False, "", "a" * 40) == (False, None)
    assert certification_mode("push", True, "", "a" * 40) == (
        True,
        "commit_marker",
    )
    assert certification_mode(
        "workflow_dispatch", False, "a" * 40, "a" * 40
    ) == (True, "workflow_dispatch")
    try:
        certification_mode("workflow_dispatch", False, "b" * 40, "a" * 40)
    except ValueError:
        pass
    else:
        raise AssertionError("dispatch certification accepted a mismatched expected_sha")

    print("godot CI scope self-tests: PASS")


if __name__ == "__main__":
    main()
