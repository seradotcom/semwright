"""Read-only reconciliation of the completed operator handoff; never rewrites its outcome."""
from pathlib import Path
import collections
import datetime
import json
import subprocess
from godot_handoff_guard import (classify_requests, check_process_identity, digest,
    inventory, observe, process_identity, projection, require, save_new)

A = Path(r"E:\Projects\worldwright-archipelago")
D = A / "evidence/integration/20261029-godot-broker/godot-bridge01"
NAME = "operator-handoff-attempt01"
ATTEMPT = D / NAME
OUT = D / "operator-handoff-reconciliation01"

def read(path):
    return json.loads(path.read_text())

def envelope(label):
    return read(ATTEMPT / (label + ".envelope.json"))

def main():
    require(__debug__, "Use ordinary Python, not optimized assertions")
    OUT.mkdir(exist_ok=False)
    result = {
        "schema": 1, "utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "read_only": True, "new_mutations_submitted": 0,
        "approval_input_automated": False, "automatic_retry": False,
        "observed_postconditions_passed": False,
        "original_runner_outcome_retained": "UNKNOWN",
        "G6": "NOT_COMPLETE", "human_edit_preservation_claimed": False,
        "art_status": "REVISE",
    }
    try:
        runner_path = D / (NAME + ".runner.json")
        proof_path = ATTEMPT / "receipt.json"
        runner, proof = read(runner_path), read(proof_path)
        cfg_path = D / (NAME + ".json")
        cfg, prepared = read(cfg_path), read(D / "prepared.json")
        baseline = read(Path(runner["preflight_receipt"]))
        require(runner["outcome"] == "UNKNOWN" and runner["passed"] is False,
                "Original uncertainty must remain visible")
        require(runner["exit_code"] == 0 and runner["execution_error"] is None
                and not runner["client_timeout"] and runner["proof_passed"]
                and runner["permitted_source_delta_only"], "Incomplete successful save prerequisites")
        require(proof["passed"] is True and proof["config_sha256"] == digest(cfg_path)
                == runner["config_sha256"], "Proof/configuration mismatch")
        result["original_evidence"] = {
            str(p.relative_to(A)): digest(p) for p in
            [runner_path, proof_path, cfg_path, Path(runner["preflight_receipt"]),
             D / (NAME + ".submission.json"), D / (NAME + ".log")]
        }
        rows = classify_requests(ATTEMPT)
        reqs = {p.name[:-len(".request.json")] for p in ATTEMPT.glob("*.request.json")}
        envs = {p.name[:-len(".envelope.json")] for p in ATTEMPT.glob("*.envelope.json")}
        require(reqs == envs and len(rows) == 35, "Unmatched or unexpected request inventory")
        unknown = [r for r in rows if r["classification"] != "KNOWN_SUCCESS"]
        require(len(unknown) == 1
                and unknown[0]["request_id"] == "7ededffa579241e2b0f794653c379725"
                and unknown[0]["classification"] == "UNKNOWN",
                "Unexpected uncertain request")
        approved = [r for r in rows if r.get("policy_decision") == "allow_after_confirmation"]
        require(len(approved) == 5 and all(r["classification"] == "KNOWN_SUCCESS"
                and r["mutation_applied"] for r in approved), "Incomplete native approvals")
        saved = [r for r in rows if r["command"] == "driver.godot.scene.save"]
        require(len(saved) == 1 and saved[0]["classification"] == "KNOWN_SUCCESS"
                and saved[0]["mutation_applied"], "Save not acknowledged")
        result["request_count"] = len(rows)
        result["native_approved_mutations"] = approved
        result["acknowledged_scene_save"] = saved[0]

        stale_req = read(ATTEMPT / "29-reject-stale-camera-configure.request.json")
        stale = envelope("29-reject-stale-camera-configure")
        before_camera = envelope("27-camera-after-save")["data"]
        after_camera = envelope("30-camera-after-stale")["data"]
        require(stale["request_id"] == stale_req["request_id"]
                and stale["error"]["code"] == "Conflict"
                and stale["error"]["outcome_known"] is False, "Error contract changed")
        require(stale_req["args"]["session"] == cfg["editor_session"]
                and stale_req["args"]["target"] == "ProofCamera"
                and stale_req["args"]["expect"]["revision"] == 10
                and stale_req["args"]["fov"] == 75, "Unexpected stale probe")
        require(before_camera["stamp"] == after_camera["stamp"]
                and before_camera["stamp"]["revision"] == 11
                and projection(before_camera["data"]) == projection(after_camera["data"])
                and after_camera["data"]["properties"]["fov"] == 60,
                "Camera postcondition differs")
        pins = {}
        for rel in ["addons/semwright/plugin.gd", "addons/semwright/ops/rendering_ops.gd"]:
            matches = [c for c in prepared["companions"] if c["destination"] == rel]
            require(len(matches) == 1 and digest(A / "game" / rel) == matches[0]["sha256"],
                    "Handler source drift")
            pins[rel] = matches[0]["sha256"]
        result["stale_camera_reconciliation"] = {
            "request_id": stale_req["request_id"], "protocol_outcome_known": False,
            "raw_classification": "UNKNOWN", "observed_postcondition": "UNCHANGED",
            "proposed_fov": 75, "observed_fov_before_and_after": 60,
            "expected_revision": 10, "observed_revision_before_and_after": 11,
            "fingerprint": after_camera["stamp"]["fingerprint"],
            "excluded_ephemeral_field": "ref", "handler_source_pins": pins,
            "source_review": "camera_configure checks _check_expect before resolving or writing the camera; revision mismatch returns synchronously through _handle_request; this branch schedules no deferred mutation",
            "inference": "Pinned precondition control flow and correlated subsequent readback support rejection before camera writes; the broker's conservative outcome_known=false is retained",
            "original_error": stale["error"],
        }
        client_check = subprocess.run(["powershell.exe", "-NoProfile", "-Command",
            "if (Get-Process -Id " + str(runner["client_pid"]) +
            " -ErrorAction SilentlyContinue) { exit 1 }; exit 0"],
            capture_output=True, timeout=20)
        require(client_check.returncode == 0, "Proof client PID still present; reconcile its identity")
        result["proof_client_absent"] = True
        result["request_completed"] = "Correlated failure response followed by acknowledged readback and tab restoration; no request replayed"
        for key in ["broker", "editor"]:
            check_process_identity(baseline[key], process_identity(baseline[key]["pid"]))

        for pin in prepared["candidate_files"]:
            require(digest(Path(pin["path"])) == pin["sha256"], "Candidate binary changed")
        cli = Path(prepared["paths"]["bin"]) / "semwright.exe"
        def call(label, command, args):
            return observe(cli, prepared["socket"], OUT / "unused-windows-ticket",
                           OUT, label, command, args)
        sessions = call("sessions-before", "session.list", {})
        require(sessions == baseline["sessions"], "Paired editor identity changed")
        args = {"session": cfg["editor_session"]}
        scene = call("main-scene", "scene.inspect", args)
        editor = call("editor-state", "editor.state", args)
        assets = call("assets-idle", "assets.status", args)
        expected_main = envelope("33-main-nodes-after")["data"]
        original_main = envelope("21-main-nodes-before")["data"]
        require(scene["data"]["scene"] == cfg["initial_scene"]
                and scene["data"]["nodes"] == expected_main["data"]["nodes"]
                == original_main["data"]["nodes"], "Main projection drift")
        require(editor["data"]["scene"] == cfg["initial_scene"]
                and editor["data"]["unsaved_scenes"] == [cfg["initial_scene"]]
                and editor["data"]["playing"] is False, "Editor state drift")
        require(assets["data"]["scanning"] is False and assets["data"]["importing"] is False,
                "Imports not idle")
        require(call("sessions-after", "session.list", {}) == sessions,
                "Session changed during reconciliation")

        original_sources = read(D / (NAME + ".source-before.json"))
        current_sources = inventory(A / "game")
        expected_sources = dict(original_sources)
        for path, delta in runner["source_delta"].items():
            require(expected_sources[path] == delta["before"], "Delta baseline mismatch")
            expected_sources[path] = delta["after"]
        require(current_sources == expected_sources, "Source changed after acknowledged save")
        require(set(runner["source_delta"]) == {cfg["supporting_scene"][6:]},
                "Unexpected actual source delta")
        require(digest(A / "game" / cfg["supporting_scene"][6:])
                == proof["result"]["supporting_scene_sha256"],
                "Saved scene differs from proof")
        save_new(OUT / "source-files.json", current_sources)
        for key in ["broker", "editor"]:
            check_process_identity(baseline[key], process_identity(baseline[key]["pid"]))
        for path, sha in result["original_evidence"].items():
            require(digest(A / path) == sha, "Original evidence changed")
        result.update(
            observed_postconditions_passed=True,
            admission="INDEPENDENT_SAVED_ARTIFACT_OBSERVATION",
            broker_reimport_and_save_verified=True,
            main_live_projection_preserved=True, main_remains_unsaved=True,
            support_saved=True, source_file_count=len(current_sources),
            source_inventory_sha256=digest(OUT / "source-files.json"),
            source_delta=runner["source_delta"],
            original_runner_passed=False, protocol_uncertainty_rewritten=False,
            current_editor=editor, sessions=sessions)
    except Exception as exc:
        result["error"] = type(exc).__name__ + ": " + str(exc)
    result["script_sha256"] = digest(Path(__file__))
    save_new(OUT / "receipt.json", result)
    print(json.dumps(result, indent=2))
    return 0 if result["observed_postconditions_passed"] else 3

if __name__ == "__main__":
    raise SystemExit(main())
