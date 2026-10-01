"""G-owned native application attacks on disposable hosted runners."""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import shutil
import re
import shlex
from typing import Any

from isolation import Enclosure, require_hosted
from lab_core import EvidenceError, compare_observation, digest, strict_json
from product import BuildCopy

LAB = Path(__file__).resolve().parent


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as stream:
        while block := stream.read(65536):
            h.update(block)
    return h.hexdigest()


def tree_hashes(root: Path) -> dict[str, str]:
    out: dict[str, str] = {}
    for path in sorted(root.rglob("*")):
        if not path.is_file() or path.is_symlink():
            continue
        relative = path.relative_to(root)
        if relative.parts and relative.parts[0] in {".godot", "__sw_saved", "export"}:
            continue
        out[relative.as_posix()] = sha256_file(path)
    return out


def godot_spec() -> dict[str, Any]:
    entities = []
    for index in range(70):
        entities.append({
            "id": f"node{index:02d}",
            "parent": None,
            "position": [float(index), 0.0, 0.0],
            "rotation": [0.0, 0.0, 0.0],
            "scale": [1.0, 1.0, 1.0],
            "groups": ["g_native"],
            "node": {"kind": "node3d"},
        })
    entities += [
        {
            "id": "nested",
            "parent": "node00",
            "position": [1.0, 2.0, 3.0],
            "rotation": [0.0, 0.0, 0.0],
            "scale": [1.0, 1.0, 1.0],
            "groups": ["g_nested"],
            "node": {"kind": "node3d"},
        },
        {
            "id": "animator",
            "parent": None,
            "position": [0.0, 0.0, 0.0],
            "rotation": [0.0, 0.0, 0.0],
            "scale": [1.0, 1.0, 1.0],
            "groups": [],
            "node": {"kind": "animator"},
        },
        {
            "id": "mesh_shared",
            "parent": None,
            "position": [0.0, 2.0, 0.0],
            "rotation": [0.0, 0.0, 0.0],
            "scale": [1.0, 1.0, 1.0],
            "groups": [],
            "node": {
                "kind": "mesh3d_material",
                "shape": {"kind": "box", "size": [1.0, 1.0, 1.0]},
                "material": {"material": "surface", "sharing": "shared"},
            },
        },
        {
            "id": "mesh_local",
            "parent": None,
            "position": [2.0, 2.0, 0.0],
            "rotation": [0.0, 0.0, 0.0],
            "scale": [1.0, 1.0, 1.0],
            "groups": [],
            "node": {
                "kind": "mesh3d_material",
                "shape": {"kind": "box", "size": [1.0, 1.0, 1.0]},
                "material": {
                    "material": "surface",
                    "sharing": "local_to_scene",
                    "color_override": [0.8, 0.2, 0.1, 1.0],
                    "roughness_override": 0.25,
                },
            },
        },
    ]
    tracks = []
    for index in range(70):
        if index == 0:
            keys = [
                {"time": float(key), "value": {"kind": "vector3", "value": [float(key), 0.0, 0.0]}}
                for key in range(70)
            ]
        else:
            keys = [{"time": 0.0, "value": {"kind": "vector3", "value": [float(index), 0.0, 0.0]}}]
        tracks.append({"entity": f"node{index:02d}", "property": "position", "keys": keys})
    return {
        "version": 1,
        "project": "g_native",
        "title": "G synthetic native Godot fixture",
        "main_scene": "arena",
        "settings": {"width": 320, "height": 180, "physics_ticks": 60},
        "inputs": [],
        "assets": [],
        "scenes": [{
            "id": "arena",
            "dimension": "three",
            "entities": entities,
            "materials": [{
                "id": "surface",
                "color": [0.2, 0.4, 0.8, 1.0],
                "roughness": 0.6,
            }],
            "animations": [{
                "id": "long",
                "animator": "animator",
                "length": 69.0,
                "looping": False,
                "tracks": tracks,
            }],
            "animation_graphs": [],
            "behavior": {
                "states": ["idle"],
                "initial_state": "idle",
                "variables": [],
                "expressions": [],
                "handlers": [],
                "timers": [],
                "signals": [],
            },
        }],
        "limits": {
            "actions_per_event": 128,
            "actions_per_tick": 2048,
            "events_per_tick": 128,
            "entities": 1024,
            "spawns_per_tick": 8,
        },
    }


def request(mode: str, source: str, nonce: str, **overrides: Any) -> dict[str, Any]:
    value: dict[str, Any] = {
        "version": 1,
        "nonce": nonce,
        "source_fingerprint": source,
        "mode": mode,
        "scene": "res://scenes/arena.tscn",
        "ticks": 0,
        "inputs": [],
        "checkpoints": [],
        "variables": [],
        "capture": False,
    }
    value.update(overrides)
    return value


def process_receipt(raw: dict[str, Any]) -> dict[str, Any]:
    return {
        "exit_code": raw["exit_code"],
        "termination_reason": raw["termination_reason"],
        "outer_process_group_gone": raw["outer_process_group_gone"],
        "canaries_unchanged": raw["canaries_unchanged"],
        "duration_seconds": raw["duration_seconds"],
        "stdout_sha256": digest(raw["stdout"]),
        "stderr_sha256": digest(raw["stderr"]),
        "stderr_tail": raw["stderr"][-2048:].decode(errors="replace"),
    }


def sound(raw: dict[str, Any], enclosure: Enclosure, *, exits: set[int] = {0}) -> bool:
    return (
        raw["exit_code"] in exits
        and raw["termination_reason"] is None
        and raw["canaries_unchanged"]
        and raw["outer_process_group_gone"]
        and enclosure.verified
    )


def run_native_process(
    enclosure: Enclosure,
    godot: Path,
    request_path: str,
    output_path: str,
) -> dict[str, Any]:
    return enclosure.run(
        [
            "/plugin/bin", "--headless", "--path", "/out/g_native",
            "--script", "/out/native_observer.gd", "--",
            "--request", request_path, "--output", output_path,
        ],
        executable=godot,
        timeout=30,
    )


def native_process_command(request_path: str, output_path: str) -> str:
    argv = [
        "/plugin/bin", "--headless", "--path", "/out/g_native",
        "--script", "/out/native_observer.gd", "--",
        "--request", request_path, "--output", output_path,
    ]
    return " ".join(shlex.quote(value) for value in argv)


def run_native_pair(
    enclosure: Enclosure,
    godot: Path,
    first_request: str,
    first_output: str,
    second_request: str,
    second_output: str,
    *,
    between: str | None = None,
) -> dict[str, Any]:
    commands = [native_process_command(first_request, first_output)]
    if between is not None:
        commands.append(between)
    commands.append(native_process_command(second_request, second_output))
    return enclosure.run(
        ["/usr/bin/bash", "-c", "set -eu; " + "; ".join(commands)],
        executable=godot,
        timeout=60,
    )


def helper(enclosure: Enclosure, binary: Path, args: list[str], *, expect_success: bool = True,
           source: Path | None = None, runtime: Path | None = None,
           timeout: float = 30.0) -> tuple[dict[str, Any], Any]:
    raw = enclosure.run(
        ["/plugin/bin", *args],
        executable=binary,
        source=source,
        runtime=runtime,
        timeout=timeout,
    )
    value = None
    if raw["stdout"]:
        try:
            value = strict_json(raw["stdout"])
        except EvidenceError:
            value = None
    if expect_success and not sound(raw, enclosure):
        raise EvidenceError("BLOCKED: G Godot helper execution failed: " + raw["stderr"][-4096:].decode(errors="replace"))
    return raw, value


def write_json(path: Path, value: Any) -> None:
    path.write_text(json.dumps(value, separators=(",", ":"), sort_keys=True) + "\n")


def dependency_sentinels(observation: dict[str, Any]) -> list[list[Any]]:
    def normalized(path: Any) -> str:
        value = str(path)
        if "::" in value:
            return "<native-subresource>"
        return value.replace("res://__sw_saved/", "res://scenes/")

    rows = [
        [
            normalized(dep.get("source", "")),
            normalized(dep.get("path", "")),
            dep.get("exists"),
            dep.get("sha256"),
        ]
        for dep in observation.get("dependencies", [])
        if isinstance(dep, dict)
    ]
    rows.sort(key=lambda row: json.dumps(row, separators=(",", ":")))
    return rows


def blocked_result(case: dict[str, Any], source_sha: str, suite_sha: str,
                   enclosure: Enclosure, reason: str) -> dict[str, Any]:
    return {
        "case_id": case["id"],
        "source_sha": source_sha,
        "suite_sha": suite_sha,
        "scope": "native_application",
        "isolation_verified": enclosure.verified,
        "outcome": "BLOCKED",
        "reason": reason,
    }


def result(case: dict[str, Any], observed: dict[str, Any], source_sha: str, suite_sha: str,
           enclosure: Enclosure, *, receipts: list[dict[str, Any]] | None = None) -> dict[str, Any]:
    success = enclosure.verified and compare_observation(observed, case["expected"])
    return {
        "case_id": case["id"],
        "source_sha": source_sha,
        "suite_sha": suite_sha,
        "scope": "native_application",
        "isolation_verified": enclosure.verified,
        "outcome": "PASS" if success else "FAIL",
        "expected": case["expected"],
        "observed": observed,
        "processes": receipts or [],
        "classification": None if success else "REQUIRES_TRIAGE_NATIVE_PRODUCT_OR_G_ORACLE",
    }


def run_godot(target: Path, source_sha: str, suite_sha: str, cases: list[dict], report: dict) -> list[dict]:
    require_hosted()
    godot = Path(os.environ.get("G_GODOT_BIN", ""))
    template = Path(os.environ.get("G_GODOT_TEMPLATE", ""))
    lock = strict_json((LAB / "targets.json").read_bytes())
    pins = lock["pins"]
    native_address_space = lock["limits"]["native_address_space_bytes"]
    native_file_size = lock["limits"]["native_file_size_bytes"]
    if not godot.is_file() or sha256_file(godot) != pins["godot_binary_sha256"]:
        raise EvidenceError("BLOCKED: pinned Godot binary unavailable or digest mismatch")
    template_sha = sha256_file(template) if template.is_file() else None
    observed_template_sha = os.environ.get("G_GODOT_TEMPLATE_SHA256")
    if (
        template_sha is None
        or observed_template_sha is None
        or not re.fullmatch(r"[0-9a-f]{64}", observed_template_sha)
        or template_sha != observed_template_sha
    ):
        raise EvidenceError("BLOCKED: extracted Godot export template provenance mismatch")

    enclosure = Enclosure(
        LAB, source_sha, address_space_bytes=native_address_space, file_size_bytes=native_file_size
    )
    build: BuildCopy | None = None
    results: list[dict] = []
    report["results"] = results
    report["native_runtime"] = {
        "application": "Godot",
        "version_pin": "4.7.2-stable",
        "binary_sha256": pins["godot_binary_sha256"],
        "template_archive_sha256": pins["godot_template_archive_sha256"],
        "template_sha256": template_sha,
        "network_during_attacks": "isolated by G enclosure",
        "address_space_bytes": native_address_space,
        "file_size_bytes": native_file_size,
    }
    try:
        report["isolation"] = enclosure.preflight()
        build = BuildCopy(target, source_sha, "godot-native")
        report["builds"] = build.builds
        build.build("godot-native-helper")

        out = enclosure.root / "out"
        spec_path = out / "spec.json"
        write_json(spec_path, godot_spec())
        shutil.copyfile(target / "integrations/godot/authoring/native_observer.gd", out / "native_observer.gd")
        report["product_native_probe_sha256"] = sha256_file(out / "native_observer.gd")

        compile_raw, compile_receipt = helper(
            enclosure,
            build.binary,
            ["compile", "/out/spec.json", "/out/g_native"],
        )
        if not isinstance(compile_receipt, dict) or compile_receipt.get("source_sha") != source_sha:
            raise EvidenceError("BLOCKED: G compile helper receipt invalid")

        project = out / "g_native"
        scene = project / "scenes" / "arena.tscn"
        if not scene.is_file():
            raise EvidenceError("BLOCKED: product compiler did not materialize the managed scene")
        source_fingerprint = sha256_file(scene)

        import_raw = enclosure.run(
            ["/plugin/bin", "--headless", "--path", "/out/g_native", "--import"],
            executable=godot,
            timeout=30,
        )
        report.setdefault("setup_processes", {})["import"] = process_receipt(import_raw)
        if not sound(import_raw, enclosure):
            raise EvidenceError(
                "BLOCKED: pinned Godot could not import G compiled fixture: "
                + import_raw["stderr"][-4096:].decode(errors="replace")
            )

        inspect_request = request("inspect", source_fingerprint, "g_native_inspect_0001")
        write_json(out / "request-inspect.json", inspect_request)
        inspect_raw = run_native_process(
            enclosure, godot, "/out/request-inspect.json", "/out/observation-inspect.json"
        )
        if not sound(inspect_raw, enclosure) or not (out / "observation-inspect.json").is_file():
            raise EvidenceError("BLOCKED: native inspect did not produce a receipt")
        observation = strict_json((out / "observation-inspect.json").read_bytes())
        decode_raw, decoded = helper(
            enclosure,
            build.binary,
            ["decode", "/out/request-inspect.json", "/out/observation-inspect.json"],
        )
        if not isinstance(decoded, dict):
            raise EvidenceError("BLOCKED: product observation decoder receipt invalid")

        nodes = observation["authored"]["nodes"]
        node_paths = {row.get("path") for row in nodes}
        results_by_id: dict[str, dict[str, Any]] = {}
        results_by_id["G-GODOT-001"] = {
            "no_failures": observation.get("failures") == [],
            "dependency_complete": observation.get("dependency_complete") is True,
            "node00_present": "node00" in node_paths,
            "animator_present": "animator" in node_paths,
        }
        inspect_text = (inspect_raw["stdout"] + inspect_raw["stderr"]).decode(errors="replace")
        results_by_id["G-GODOT-013"] = {
            "process_exit_zero": inspect_raw["exit_code"] == 0
            and inspect_raw["termination_reason"] is None,
            "observer_reported_failures_empty": observation.get("failures") == [],
            "script_errors_absent": "SCRIPT ERROR:" not in inspect_text,
        }

        first_raw, first_page = helper(
            enclosure,
            build.binary,
            ["track-page", "/out/request-inspect.json", "/out/observation-inspect.json", "-"],
        )
        if not isinstance(first_page, dict) or not isinstance(first_page.get("next_cursor"), str):
            raise EvidenceError("BLOCKED: first native track page receipt invalid")
        second_raw, second_page = helper(
            enclosure,
            build.binary,
            ["track-page", "/out/request-inspect.json", "/out/observation-inspect.json", first_page["next_cursor"]],
        )
        results_by_id["G-GODOT-002"] = {
            "total": first_page.get("total"),
            "first": len(first_page.get("tracks", [])),
            "last": len(second_page.get("tracks", [])) if isinstance(second_page, dict) else None,
            "final_cursor_none": isinstance(second_page, dict) and second_page.get("next_cursor") is None,
        }

        animations = observation["authored"]["animations"]
        long_animation = next(
            (animation for animation in animations if any(track.get("key_count") == 70 for track in animation.get("tracks", []))),
            None,
        )
        if long_animation is None:
            raise EvidenceError("BLOCKED: native observer did not expose the long animation")
        long_track = next(track for track in long_animation["tracks"] if track.get("key_count") == 70)
        key_args = [
            "/out/request-inspect.json", "/out/observation-inspect.json",
            long_animation["player"], long_animation["library"], long_animation["name"],
            str(long_track["index"]),
        ]
        key_first_raw, key_first = helper(enclosure, build.binary, ["key-page", *key_args, "-"])
        if not isinstance(key_first, dict) or not isinstance(key_first.get("next_cursor"), str):
            raise EvidenceError("BLOCKED: first native key page receipt invalid")
        key_second_raw, key_second = helper(
            enclosure, build.binary, ["key-page", *key_args, key_first["next_cursor"]]
        )
        results_by_id["G-GODOT-003"] = {
            "total": key_first.get("total"),
            "first": len(key_first.get("keys", [])),
            "last": len(key_second.get("keys", [])) if isinstance(key_second, dict) else None,
            "final_cursor_none": isinstance(key_second, dict) and key_second.get("next_cursor") is None,
        }

        nested = next((row for row in nodes if row.get("path") == "node00/nested"), {})
        material_refs = [
            row.get("resource", {})
            for row in observation["authored"].get("resources", [])
            if row.get("resource", {}).get("class") == "StandardMaterial3D"
        ]
        results_by_id["G-GODOT-004"] = {
            "parent_points_to_node00": nested.get("parent") == "node00",
            "parent_owner_distinct": nested.get("parent") is not None
            and nested.get("owner") is not None
            and nested.get("parent") != nested.get("owner"),
            "shared_and_local_materials": any(r.get("local_to_scene") is False for r in material_refs)
            and any(r.get("local_to_scene") is True for r in material_refs),
        }

        before_save = tree_hashes(project)
        save_request = request("save_candidate", source_fingerprint, "g_native_save_000001")
        reopen_request = request("reopen_candidate", source_fingerprint, "g_native_reopen_0001")
        write_json(out / "request-save.json", save_request)
        write_json(out / "request-reopen.json", reopen_request)
        persistence_pair_raw = run_native_pair(
            enclosure,
            godot,
            "/out/request-save.json",
            "/out/observation-save.json",
            "/out/request-reopen.json",
            "/out/observation-reopen.json",
        )
        save_raw = persistence_pair_raw
        reopen_raw = persistence_pair_raw
        if (
            not sound(persistence_pair_raw, enclosure)
            or not (out / "observation-save.json").is_file()
            or not (out / "observation-reopen.json").is_file()
        ):
            diagnostic: dict[str, Any] = {
                "exit_code": persistence_pair_raw["exit_code"],
                "termination_reason": persistence_pair_raw["termination_reason"],
                "save_exists": (out / "observation-save.json").is_file(),
                "reopen_exists": (out / "observation-reopen.json").is_file(),
                "stderr_tail": persistence_pair_raw["stderr"][-2048:].decode(errors="replace"),
            }
            for label, path in [
                ("save", out / "observation-save.json"),
                ("reopen", out / "observation-reopen.json"),
            ]:
                if path.is_file():
                    try:
                        receipt = strict_json(path.read_bytes())
                        diagnostic[label] = {
                            key: receipt.get(key)
                            for key in ("mode", "process_id", "loaded_scene", "failures")
                        }
                    except EvidenceError:
                        diagnostic[label] = {"receipt": "invalid_json"}
            raise EvidenceError(
                "BLOCKED: native save/reopen pair diagnostic: "
                + json.dumps(diagnostic, separators=(",", ":"), sort_keys=True)
            )
        save_obs = strict_json((out / "observation-save.json").read_bytes())
        reopen_obs = strict_json((out / "observation-reopen.json").read_bytes())
        after_save = tree_hashes(project)
        candidate = project / "__sw_saved" / "arena.tscn"
        results_by_id["G-GODOT-005"] = {
            "source_unchanged": sha256_file(scene) == source_fingerprint,
            "authored_files_unchanged": before_save == after_save,
            "candidate_created": candidate.is_file()
            and save_obs.get("candidate_sha256") == sha256_file(candidate),
        }

        writer_sentinels = dependency_sentinels(save_obs)
        reader_sentinels = dependency_sentinels(reopen_obs)
        report["persistence_diagnostic"] = {
            "writer_count": len(writer_sentinels),
            "reader_count": len(reader_sentinels),
            "equal": writer_sentinels == reader_sentinels,
            "writer_only": [row for row in writer_sentinels if row not in reader_sentinels][:32],
            "reader_only": [row for row in reader_sentinels if row not in writer_sentinels][:32],
        }
        persist_raw, persist = helper(
            enclosure,
            build.binary,
            [
                "persistence",
                "/out/request-save.json",
                "/out/observation-save.json",
                "/out/request-reopen.json",
                "/out/observation-reopen.json",
            ],
            expect_success=False,
        )
        baseline_persistence_ok = (
            sound(persist_raw, enclosure)
            and isinstance(persist, dict)
            and persist.get("ok") is True
        )
        results_by_id["G-GODOT-006"] = {
            "fresh_process": save_obs.get("process_id") != reopen_obs.get("process_id"),
            "persistence_verified": baseline_persistence_ok,
        }

        blocked_cases: dict[str, str] = {}
        tamper_raw = None
        reject_raw = None
        if baseline_persistence_ok:
            dependencies = save_obs.get("dependencies", [])
            external = next(
                (
                    dep for dep in dependencies
                    if isinstance(dep.get("path"), str)
                    and dep["path"].startswith("res://")
                    and dep["path"] not in {"res://scenes/arena.tscn", "res://__sw_saved/arena.tscn"}
                    and (project / dep["path"][6:]).is_file()
                ),
                None,
            )
            if external is None:
                blocked_cases["G-GODOT-007"] = (
                    "baseline passed but no mutable external dependency sentinel was available"
                )
            else:
                external_path = project / external["path"][6:]
                external_before = external_path.read_bytes()
                if candidate.is_file():
                    candidate.unlink()
                tamper_save_request = request(
                    "save_candidate", source_fingerprint, "g_native_save_tamper_01"
                )
                tamper_request = request(
                    "reopen_candidate", source_fingerprint, "g_native_reopen_0002"
                )
                write_json(out / "request-save-tamper.json", tamper_save_request)
                write_json(out / "request-reopen-tamper.json", tamper_request)
                external_enclosure_path = "/out/" + external_path.relative_to(out).as_posix()
                tamper_command = (
                    "printf %s "
                    + shlex.quote("\n# g-synthetic-dependency-tamper\n")
                    + " >> "
                    + shlex.quote(external_enclosure_path)
                )
                tamper_raw = run_native_pair(
                    enclosure,
                    godot,
                    "/out/request-save-tamper.json",
                    "/out/observation-save-tamper.json",
                    "/out/request-reopen-tamper.json",
                    "/out/observation-reopen-tamper.json",
                    between=tamper_command,
                )
                if (
                    not sound(tamper_raw, enclosure)
                    or not (out / "observation-save-tamper.json").is_file()
                    or not (out / "observation-reopen-tamper.json").is_file()
                ):
                    blocked_cases["G-GODOT-007"] = (
                        "tampered dependency pair did not produce valid native receipts"
                    )
                else:
                    reject_raw, _ = helper(
                        enclosure,
                        build.binary,
                        [
                            "persistence",
                            "/out/request-save-tamper.json",
                            "/out/observation-save-tamper.json",
                            "/out/request-reopen-tamper.json",
                            "/out/observation-reopen-tamper.json",
                        ],
                        expect_success=False,
                    )
                    results_by_id["G-GODOT-007"] = {
                        "dependency_tamper_rejected": sound(reject_raw, enclosure, exits={1}),
                        "reason_seen": b"External native dependency sentinel changed"
                        in reject_raw["stderr"],
                    }
                external_path.write_bytes(external_before)
        else:
            blocked_cases["G-GODOT-007"] = (
                "baseline persistence is not green; dependency-tamper claim cannot be isolated"
            )

        bad_nonplay = request(
            "inspect", source_fingerprint, "g_native_badinput_01",
            ticks=1, inputs=[{"tick": 1, "action": "ghost", "pressed": False}],
        )
        write_json(out / "request-bad-nonplay.json", bad_nonplay)
        bad_nonplay_raw = run_native_process(
            enclosure, godot, "/out/request-bad-nonplay.json", "/out/observation-bad-nonplay.json"
        )
        bad_nonplay_text = (bad_nonplay_raw["stdout"] + bad_nonplay_raw["stderr"]).decode(errors="replace")
        results_by_id["G-GODOT-008"] = {
            "rejected": sound(bad_nonplay_raw, enclosure, exits={2}),
            "reason_seen": "nonplay_request_may_not_inject_input" in bad_nonplay_text,
        }

        bad_unknown = request("inspect", source_fingerprint, "g_native_badfield_001")
        bad_unknown["authority"] = "admin"
        write_json(out / "request-bad-field.json", bad_unknown)
        bad_unknown_raw = run_native_process(
            enclosure, godot, "/out/request-bad-field.json", "/out/observation-bad-field.json"
        )
        bad_unknown_text = (bad_unknown_raw["stdout"] + bad_unknown_raw["stderr"]).decode(errors="replace")
        results_by_id["G-GODOT-009"] = {
            "rejected": sound(bad_unknown_raw, enclosure, exits={2}),
            "reason_seen": "unknown_request_field" in bad_unknown_text,
        }

        mutated = json.loads(json.dumps(observation))
        mutated["authored"]["animations"][0]["tracks"][-1]["path"] = "node69:changed"
        write_json(out / "observation-mutated.json", mutated)
        stale_raw, _ = helper(
            enclosure,
            build.binary,
            [
                "track-page", "/out/request-inspect.json", "/out/observation-mutated.json",
                first_page["next_cursor"],
            ],
            expect_success=False,
        )
        results_by_id["G-GODOT-010"] = {
            "stale_cursor_rejected": sound(stale_raw, enclosure, exits={1}),
        }

        marker = "G_SYNTHETIC_DO_NOT_EXPORT_7f31b5"
        (project / "authoring-spec.json").write_text(marker + "\n")
        addon = project / "addons" / "g_probe"
        addon.mkdir(parents=True, exist_ok=True)
        (addon / "marker.gd").write_text("# " + marker + "\n")
        runner_artifacts = out / "runner-artifacts"
        template_home = (
            runner_artifacts
            / ".semwright-home"
            / "data"
            / "godot"
            / "export_templates"
            / "4.7.2.stable"
        )
        template_home.mkdir(parents=True)
        template_copy = template_home / "linux_release.x86_64"
        shutil.copyfile(template, template_copy)
        template_copy.chmod(0o755)
        export_raw, export_receipt = helper(
            enclosure,
            build.binary,
            [
                "runner-export",
                "/out/g_native",
                "/out/runner-artifacts",
                pins["godot_binary_sha256"],
            ],
            runtime=godot,
            timeout=30,
            expect_success=False,
        )
        binary = runner_artifacts / "g_native.x86_64"
        result_value = export_receipt.get("result") if isinstance(export_receipt, dict) else None
        exported = (
            sound(export_raw, enclosure)
            and isinstance(result_value, dict)
            and result_value.get("success") is True
            and result_value.get("artifact") == "g_native.x86_64"
            and binary.is_file()
        )
        mode = (binary.stat().st_mode & 0o777) if binary.is_file() else None
        report["export_diagnostic"] = {
            "route": "Runner::execute(driver.godot.export.build)",
            "artifact": result_value.get("artifact") if isinstance(result_value, dict) else None,
            "mode": mode,
            "executable": binary.is_file() and os.access(binary, os.X_OK),
            "helper_exit_code": export_raw["exit_code"],
            "helper_termination_reason": export_raw["termination_reason"],
            "helper_stderr_tail": export_raw["stderr"][-2048:].decode(errors="replace"),
        }
        results_by_id["G-GODOT-011"] = {"exported": exported}
        export_bytes = binary.read_bytes() if binary.is_file() else b""
        forbidden = [
            marker.encode(),
            b"authoring-spec.json",
            b"native_observer.gd",
            b"SEMWRIGHT_GODOT_PORT",
            b"GH_TOKEN",
        ]
        launch_script = (
            "set -eu; "
            "cp /out/runner-artifacts/g_native.x86_64 /tmp/g_native.x86_64; "
            "chmod 0755 /tmp/g_native.x86_64; "
            "exec /usr/bin/timeout 5 /tmp/g_native.x86_64 --headless"
        )
        launch_raw = enclosure.run(
            ["/usr/bin/bash", "-c", launch_script],
            timeout=8,
        ) if exported else {
            "exit_code": -1, "termination_reason": "not_exported", "canaries_unchanged": True,
            "outer_process_group_gone": True, "stdout": b"", "stderr": b"", "duration_seconds": 0,
        }
        launch_text = (launch_raw["stdout"] + launch_raw["stderr"]).decode(errors="replace")
        results_by_id["G-GODOT-012"] = {
            "authoring_markers_absent": all(item not in export_bytes for item in forbidden),
            "standalone_without_broker": launch_raw["exit_code"] in {0, 124}
            and launch_raw["termination_reason"] is None
            and "SCRIPT ERROR:" not in launch_text
            and "Parse Error:" not in launch_text,
        }

        case_map = {case["id"]: case for case in cases}
        execution_map = {
            "G-GODOT-001": [import_raw, inspect_raw, decode_raw],
            "G-GODOT-002": [first_raw, second_raw],
            "G-GODOT-003": [key_first_raw, key_second_raw],
            "G-GODOT-004": [inspect_raw],
            "G-GODOT-005": [persistence_pair_raw],
            "G-GODOT-006": [persistence_pair_raw, persist_raw],
            "G-GODOT-007": [raw for raw in [tamper_raw, reject_raw] if raw is not None],
            "G-GODOT-008": [bad_nonplay_raw],
            "G-GODOT-009": [bad_unknown_raw],
            "G-GODOT-010": [stale_raw],
            "G-GODOT-011": [export_raw],
            "G-GODOT-012": [export_raw, launch_raw],
            "G-GODOT-013": [inspect_raw],
        }
        for case in cases:
            if case["id"] in blocked_cases:
                results.append(
                    blocked_result(
                        case,
                        source_sha,
                        suite_sha,
                        enclosure,
                        blocked_cases[case["id"]],
                    )
                )
                continue
            observed = results_by_id.get(case["id"])
            if observed is None:
                raise EvidenceError("BLOCKED: native case observation missing")
            row = result(
                case,
                observed,
                source_sha,
                suite_sha,
                enclosure,
                receipts=[process_receipt(raw) for raw in execution_map.get(case["id"], [])],
            )
            results.append(row)
        return results
    finally:
        report["isolation"] = enclosure.proof
        enclosure_clean = enclosure.close()
        build_clean = True if build is None else build.close()
        report["cleanup_verified"] = enclosure_clean and build_clean
        report["target_checkout_unchanged"] = build_clean


def run_native(lane: str, target: Path, source_sha: str, suite_sha: str,
               cases: list[dict], report: dict) -> list[dict]:
    if lane == "godot-native":
        return run_godot(target, source_sha, suite_sha, cases, report)
    raise EvidenceError("BLOCKED: independent native lane not yet implemented")
