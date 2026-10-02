#!/usr/bin/env python3
"""Portable self-tests for affected-lane selection."""

from select_ci_scope import (
    HOST_EXTRA_LANES,
    LANES,
    NATIVE_SCENARIOS,
    certification_mode,
    host_lanes_for_changed_lines,
    host_scenarios_for_changed_lines,
    native_scenarios_for_selection,
    native_subgates,
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
        ["crates/driver-godot/tests/contracts.rs"],
        {"native"},
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
    assert host_lanes_for_changed_lines(host_source, {3}) == {"native"}
    assert host_lanes_for_changed_lines(host_source, {4}) == {"persistence"}
    assert host_lanes_for_changed_lines(host_source, {5}) == {"export"}
    assert host_lanes_for_changed_lines(host_source, {6}) == {"cross_app"}
    assert host_lanes_for_changed_lines(host_source, {1}) == set(HOST_EXTRA_LANES)
    assert host_lanes_for_changed_lines(host_source, set()) == set(HOST_EXTRA_LANES)
    assert host_scenarios_for_changed_lines(host_source, {3}) == {
        "typed_transform_and_reparent_actions_round_trip_natively"
    }
    assert host_scenarios_for_changed_lines(host_source, {4}) == set()
    assert host_scenarios_for_changed_lines(host_source, {6}) == {
        "driver_host_handshake_control_reaches_capabilities"
    }
    assert host_scenarios_for_changed_lines(host_source, {1}) == set(NATIVE_SCENARIOS)

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

    assert native_subgates(
        ["crates/driver-godot/tests/authoring_host.rs"], False
    ) == (False, False)
    assert native_subgates(
        ["integrations/godot/tests/scene_save_contract.gd"], False
    ) == (True, False)
    assert native_subgates(
        ["crates/driver-godot/src/runner.rs"], False
    ) == (False, True)
    assert native_subgates(
        ["crates/driver-host/src/lib.rs"], False
    ) == (True, True)
    assert native_subgates(
        [".github/workflows/godot-authoring.yml"], False
    ) == (True, True)
    assert native_subgates([], True) == (True, True)

    assert native_scenarios_for_selection(
        ["crates/driver-godot/tests/contracts.rs"], False
    ) == set()
    assert native_scenarios_for_selection(
        ["integrations/godot/tests/scene_save_contract.gd"], False
    ) == set()
    assert native_scenarios_for_selection(
        ["crates/driver-godot/src/runner.rs"], False
    ) == set(NATIVE_SCENARIOS)
    assert native_scenarios_for_selection(
        ["crates/driver-godot/tests/authoring_host.rs"],
        False,
        host_scenarios={"typed_transform_and_reparent_actions_round_trip_natively"},
    ) == {"typed_transform_and_reparent_actions_round_trip_natively"}
    assert native_scenarios_for_selection(
        [".github/workflows/godot-authoring.yml"],
        False,
        workflow_extra_lanes={"native"},
    ) == set(NATIVE_SCENARIOS)
    assert native_scenarios_for_selection([], True) == set(NATIVE_SCENARIOS)

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
