#!/usr/bin/env python3
"""Portable self-tests for affected-lane selection."""

from select_ci_scope import LANES, certification_mode, select


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
        set(LANES),
    )
    expect(
        ["crates/driver-host/src/lib.rs"],
        set(LANES),
    )
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
