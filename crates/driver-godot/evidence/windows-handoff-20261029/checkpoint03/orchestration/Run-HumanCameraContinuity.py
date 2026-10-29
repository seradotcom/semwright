"""One-shot, revision-checked preservation of a real owner camera edit."""
from pathlib import Path
import copy
import datetime
import json
import subprocess
import sys
from godot_handoff_guard import (check_process_identity, digest, inventory, observe,
    process_identity, projection, require, save_new)

A = Path(r"E:\Projects\worldwright-archipelago")
D = A / "evidence/integration/20261029-godot-broker/godot-bridge01"
RESUME = D / "human-camera-resume-20261029T060215Z"
OUT = D / "human-camera-continuity01"

def read(path):
    return json.loads(Path(path).read_text(encoding="utf-8"))

def main():
    require(__debug__, "Ordinary Python required")
    OUT.mkdir(exist_ok=False)
    result = {"schema": 1, "utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "passed": False, "outcome": "NOT_SUBMITTED", "automatic_retry": False,
        "approval_input_automated": False, "policy_modified": False,
        "human_report": "Owner replied listo after personally editing the requested camera",
        "original_handoff_outcome": "UNKNOWN", "G6": "NOT_COMPLETE", "art_status": "REVISE"}
    commands = []
    baseline_sources = None
    try:
        prepared = read(D / "prepared.json")
        cfg = read(D / "operator-handoff-attempt01.json")
        prior = read(D / "operator-handoff-attempt01.runner.json")
        identities = read(Path(prior["preflight_receipt"]))
        reconciled = read(RESUME / "reconciliation-readonly.json")
        resume = read(RESUME / "resume.json")
        cli = Path(prepared["paths"]["bin"]) / "semwright.exe"
        session = cfg["editor_session"]
        scene_path = cfg["supporting_scene"]
        source_path = A / "game" / scene_path[6:]
        require(prior["outcome"] == "UNKNOWN" and prior["passed"] is False,
                "Historical uncertainty must remain intact")
        result["historical_runner_sha256"] = digest(D / "operator-handoff-attempt01.runner.json")
        for pin in prepared["candidate_files"]:
            require(digest(pin["path"]) == pin["sha256"], "Candidate binary changed")
        for key in ("broker", "editor"):
            check_process_identity(identities[key], process_identity(identities[key]["pid"]))
        # The external save of main only adds native UID metadata. Preserve those bytes.
        require("main.tscn" in reconciled["source_delta"]
                and set(reconciled["source_delta"]) <= {"main.tscn", scene_path[6:]}, "Unreconciled source change")
        baseline_sources = inventory(A / "game")
        require(baseline_sources == read(RESUME / "source-current.json"), "Source changed after reconciliation")
        save_new(OUT / "source-before.json", baseline_sources)
        result["external_change_reconciliation"] = {
            "path": str(RESUME / "reconciliation-readonly.json"),
            "sha256": digest(RESUME / "reconciliation-readonly.json"),
            "main_source_change": reconciled["source_delta"]["main.tscn"],
            "main_current_status": "Saved externally before this run; preserve current state",
            "added_camera": "ProofCamera2 observed and preserved; not created by this runner",
            "support_edit_initially_persisted": False,
            "support_saved_during_readonly_reconciliation": scene_path[6:] in reconciled["source_delta"]}

        def ro(label, cmd, args):
            return observe(cli, prepared["socket"], OUT / "unused-windows-ticket",
                           OUT, label, cmd, args)

        def state(prefix):
            scene = ro(prefix + "-scene", "scene.inspect", {"session": session})
            cameras = {name: ro(prefix + "-" + name, "node.inspect",
                {"session": session, "path": name}) for name in ("ProofCamera", "ProofCamera2")}
            target = ro(prefix + "-target", "node.inspect", {"session": session, "path": "FaroThreshold"})
            editor = ro(prefix + "-editor", "editor.state", {"session": session})
            assets = ro(prefix + "-assets", "assets.status", {"session": session})
            require(scene["data"]["scene"] == scene_path and editor["data"]["scene"] == scene_path,
                    "Active support scene changed")
            require(editor["data"]["playing"] is False and assets["data"]["importing"] is False
                    and assets["data"]["scanning"] is False, "Editor is busy")
            require(all(v["stamp"] == scene["stamp"] for v in list(cameras.values()) + [target, editor, assets]),
                    "Concurrent scene revision")
            return {"scene": scene, "cameras": cameras, "target": target, "editor": editor, "assets": assets}

        def mutate(label, command, args):
            full = "driver.godot." + command
            # Exclusive intent marker precedes each non-idempotent request. No retry exists.
            intent = {"command": full, "args": args, "automatic_retry": False,
                      "native_approval_response_automated": False}
            save_new(OUT / (label + ".intent.json"), intent)
            row = {"label": label, "command": full, "classification": "UNKNOWN"}
            commands.append(row)
            result["outcome"] = "UNKNOWN"
            argv = [str(cli), "--socket", prepared["socket"], "--json", "execute", full,
                    "--args-json", json.dumps(args, separators=(",", ":"))]
            try:
                proc = subprocess.run(argv, capture_output=True, timeout=180)
                raw, err = proc.stdout, proc.stderr
            except subprocess.TimeoutExpired as exc:
                (OUT / (label + ".stdout.partial")).write_bytes(exc.stdout or b"")
                (OUT / (label + ".stderr.partial")).write_bytes(exc.stderr or b"")
                raise RuntimeError("Client timeout; broker outcome UNKNOWN; no further mutation") from exc
            with (OUT / (label + ".envelope.json")).open("xb") as f:
                f.write(raw)
            with (OUT / (label + ".stderr.log")).open("xb") as f:
                f.write(err)
            envelope = json.loads(raw)
            row.update(request_id=envelope.get("request_id"), exit_code=proc.returncode)
            require(envelope.get("command") == full and isinstance(envelope.get("request_id"), str),
                    "Uncorrelated mutation response")
            if envelope.get("ok") is False:
                error = envelope.get("error") or {}
                row["classification"] = "KNOWN_REJECTION" if error.get("outcome_known") is True else "UNKNOWN"
                row["error"] = error
                raise RuntimeError("Mutation rejected; observe before any later operation")
            execution = envelope.get("execution") or {}
            require(proc.returncode == 0 and envelope.get("ok") is True and not envelope.get("error")
                    and execution.get("backend") == "driver:godot"
                    and execution.get("policy_decision") in ("allow", "allow_after_confirmation")
                    and execution.get("provenance", {}).get("source") == "driver"
                    and envelope.get("data", {}).get("applied") is True,
                    "Mutation lacks an acknowledged first-party applied result")
            row.update(classification="KNOWN_SUCCESS", applied=True,
                       policy_decision=execution["policy_decision"])
            return envelope["data"]

        sessions = ro("00-sessions", "session.list", {})
        require(sessions == resume["sessions"] == identities["sessions"], "Session identity changed")
        human = state("01-human")
        require(human["scene"]["data"]["nodes"] == resume["scene"]["data"]["nodes"], "Unreconciled live edit")
        require(human["editor"]["data"]["unsaved_scenes"] in ([], [scene_path]), "Unexpected unsaved state")
        human_cameras = {n: projection(v["data"]) for n, v in human["cameras"].items()}
        require(human_cameras["ProofCamera"]["properties"]["fov"] == 70.0
                and human_cameras["ProofCamera"]["properties"]["fov"] != 60.0, "Human edit not observed")
        require(human["target"]["data"]["class"] == "Node3D"
                and human["target"]["data"]["observed"]["position"] == [0, 0, 0], "Managed target drift")
        save_new(OUT / "human-baseline.json", {"scene": human["scene"], "cameras": human_cameras,
                 "target": projection(human["target"]["data"]), "sessions": sessions})
        # Preserve the owner's actual unsaved state before changing a separate node.
        if human["editor"]["data"]["unsaved_scenes"]:
            mutate("02-save-human-state", "scene.save", {"session": session,
                   "expect": human["scene"]["stamp"], "dry_run": False, "save_external_resources": True})
            saved_human = state("03-human-saved")
            result["human_state_saved_by"] = "Broker before the separate agent edit"
        else:
            saved_human = human
            result["human_state_saved_by"] = "Already saved externally before agent mutation"
        require(saved_human["scene"]["data"]["nodes"] == human["scene"]["data"]["nodes"], "Saving altered scene projection")
        require(all(projection(saved_human["cameras"][n]["data"]) == human_cameras[n] for n in human_cameras),
                "Saving altered a human camera")
        require(saved_human["editor"]["data"]["unsaved_scenes"] == [], "Human support scene not saved")
        with (OUT / "human-saved-before-agent.tscn").open("xb") as f:
            f.write(source_path.read_bytes())
        result["human_saved_before_agent_sha256"] = digest(source_path)
        # Noncamera instance translation, using the current post-save revision.
        mutate("04-patch-threshold", "node.patch", {"session": session, "target": "FaroThreshold",
            "properties": [{"name": "position", "value": {"$type": "Vector3", "value": [0.5, 0, 0]}}],
            "expect": saved_human["scene"]["stamp"], "dry_run": False})
        patched = state("05-after-patch")
        expected_nodes = copy.deepcopy(human["scene"]["data"]["nodes"])
        targets = [n for n in expected_nodes if n["path"] == "FaroThreshold"]
        require(len(targets) == 1, "Ambiguous target")
        targets[0]["stored"]["transform"]["value"][9] = 0.5
        require(patched["scene"]["data"]["nodes"] == expected_nodes, "Unexpected scene edit")
        require(patched["target"]["data"]["observed"]["position"] == [0.5, 0, 0], "Translation not observed")
        require(all(projection(patched["cameras"][n]["data"]) == human_cameras[n] for n in human_cameras),
                "Agent edit altered a camera")
        require(patched["editor"]["data"]["unsaved_scenes"] == [scene_path], "Unexpected post-patch unsaved state")
        mutate("06-save-agent-state", "scene.save", {"session": session,
               "expect": patched["scene"]["stamp"], "dry_run": False, "save_external_resources": True})
        final = state("07-final")
        require(final["scene"]["data"]["nodes"] == expected_nodes, "Final scene projection drift")
        require(all(projection(final["cameras"][n]["data"]) == human_cameras[n] for n in human_cameras),
                "Final save altered a camera")
        require(final["editor"]["data"]["unsaved_scenes"] == [], "Final support scene not saved")
        require(ro("08-sessions-after", "session.list", {}) == sessions, "Session changed")
        final_sources = inventory(A / "game")
        changes = {k: {"before": baseline_sources.get(k), "after": final_sources.get(k)}
                   for k in baseline_sources.keys() | final_sources.keys() if baseline_sources.get(k) != final_sources.get(k)}
        require(set(changes) == {scene_path[6:]}, "Unexpected agent source delta")
        save_new(OUT / "source-after.json", final_sources)
        with (OUT / "human-and-agent-saved.tscn").open("xb") as f:
            f.write(source_path.read_bytes())
        expectation = {"schema": 1, "project_root": str(A / "game"), "scene": scene_path,
            "scene_sha256": digest(source_path), "godot_executable": reconciled["godot_executable"],
            "godot_sha256": reconciled["godot_sha256"],
            "cameras": {n: v["properties"] for n, v in human_cameras.items()},
            "managed_target": {"path": "FaroThreshold", "position": [0.5, 0, 0]},
            "assets": cfg["assets"], "node_paths": [n["path"] for n in expected_nodes]}
        save_new(OUT / "persistence-expectation.json", expectation)
        for key in ("broker", "editor"):
            check_process_identity(identities[key], process_identity(identities[key]["pid"]))
        require(digest(D / "operator-handoff-attempt01.runner.json") == result["historical_runner_sha256"],
                "Historical uncertainty receipt changed")
        result.update(passed=True, outcome="KNOWN_SUCCESS", cameras_preserved=human_cameras,
            source_delta=changes, source_files=len(final_sources),
            source_scene_sha256=digest(source_path), expectation_sha256=digest(OUT / "persistence-expectation.json"),
            native_reopen="PENDING", main_bytes_preserved=True, main_live_projection="NOT_REOBSERVED_INACTIVE",
            owner_blender_untouched=True, camera_mutations_submitted=0)
    except Exception as exc:
        result["error"] = type(exc).__name__ + ": " + str(exc)
        if commands and all(row["classification"] == "KNOWN_SUCCESS" for row in commands):
            result["outcome"] = "PARTIAL_KNOWN"
    result["mutations"] = commands
    result["script_sha256"] = digest(Path(__file__))
    save_new(OUT / "receipt.json", result)
    print(json.dumps({k: v for k, v in result.items() if k != "cameras_preserved"}, indent=2), flush=True)
    return 0 if result["passed"] else 3

if __name__ == "__main__":
    raise SystemExit(main())
