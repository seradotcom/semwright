"""Read-only guard for the existing native-operator Godot handoff.

This module never approves, submits a mutation, restarts a process, or repairs ACLs.
The inactive main scene's live projection requires the later operator-approved tab
switch; disk identity and an unsaved flag cannot establish that projection.
"""
from pathlib import Path
import datetime
import hashlib
import json
import ntpath
import os
import subprocess
import uuid

READ_COMMANDS = frozenset({
    "session.list", "project.inspect", "scene.inspect", "node.inspect",
    "editor.state", "assets.status",
})
SOURCE = "crates/driver-blender/tests/windows_broker_godot_handoff.rs"


class PreflightError(RuntimeError):
    pass


def require(condition, message):
    if not condition:
        raise PreflightError(message)


def digest(path):
    with Path(path).open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def save_new(path, value):
    with Path(path).open("x", encoding="utf-8", newline="\n") as stream:
        json.dump(value, stream, indent=2)
        stream.write("\n")
        stream.flush()
        os.fsync(stream.fileno())


def is_reparse(path):
    return path.is_symlink() or bool(
        getattr(path.lstat(), "st_file_attributes", 0) & 0x400
    )


def inventory(root):
    """Bounded source inventory; never descend into a junction or symlink."""
    root = Path(root)
    require(root.is_dir() and not is_reparse(root), "Unsafe source root")
    rows = {}
    total = 0
    def walk_error(error):
        raise error
    for current, dirs, files in os.walk(root, followlinks=False, onerror=walk_error):
        current = Path(current)
        for name in list(dirs):
            child = current / name
            if child.relative_to(root).parts == (".godot",):
                dirs.remove(name)  # Explicit editor cache/state exclusion.
                continue
            require(not is_reparse(child), "Reparse source directory: " + str(child))
        for name in sorted(files):
            child = current / name
            require(not is_reparse(child), "Reparse source file: " + str(child))
            size = child.stat().st_size
            total += size
            require(total < 2 * 1024**3 and len(rows) < 10000,
                    "Source inventory budget exceeded")
            rows[child.relative_to(root).as_posix()] = {
                "bytes": size, "sha256": digest(child),
            }
    return dict(sorted(rows.items()))


def process_identity(pid):
    code = (
        "$p=Get-Process -Id " + str(int(pid)) + " -ErrorAction Stop; "
        "@{pid=$p.Id;path=$p.Path;"
        "created=$p.StartTime.ToUniversalTime().ToString('o');"
        "session=$p.SessionId} | ConvertTo-Json -Compress"
    )
    child = subprocess.run(
        ["powershell.exe", "-NoProfile", "-Command", code],
        capture_output=True, text=True, timeout=20,
    )
    require(child.returncode == 0, "Required process unavailable: " + str(pid))
    return json.loads(child.stdout)


def check_process_identity(expected, observed):
    require(observed["pid"] == expected["pid"], "Process PID changed")
    require(observed["created"] == expected["created"], "Process birth changed")
    require(ntpath.normcase(ntpath.normpath(observed["path"])) ==
            ntpath.normcase(ntpath.normpath(expected["path"])), "Process path changed")
    require(observed["session"] == expected["session"], "Windows session changed")


def projection(value):
    return {key: item for key, item in value.items() if key != "ref"}


def validate_live(cfg, baseline, observations):
    rows = observations["sessions"]["sessions"]
    require(rows == baseline["sessions"]["sessions"],
            "Editor session, generation, version or project changed")
    require(len(rows) == 1 and rows[0]["session"] == cfg["editor_session"]
            and rows[0]["project"] == cfg["project_id"], "Unexpected paired project")
    scene = observations["scene"]
    require(scene["stamp"] == cfg["reconciled_stamp"], "Fixture revision changed")
    require(scene["data"]["scene"] == cfg["supporting_scene"],
            "Active fixture changed")
    require(scene["data"]["nodes"] == baseline["scene"]["data"]["nodes"],
            "Fixture projection changed")
    camera = observations["camera"]
    require(camera["stamp"] == cfg["reconciled_stamp"], "Camera revision changed")
    require(projection(camera["data"]) == projection(cfg["reconciled_camera_data"]),
            "Fixture camera changed")
    editor = observations["editor"]["data"]
    require(editor["scene"] == cfg["supporting_scene"], "Editor active tab changed")
    require(sorted(editor["unsaved_scenes"]) ==
            sorted([cfg["initial_scene"], cfg["supporting_scene"]]),
            "Protected unsaved scenes changed")
    require(editor["playing"] is False, "Editor game loop is running")
    require(observations["project"]["data"]["scene"] == cfg["supporting_scene"],
            "Project observation changed")
    assets = observations["assets"]["data"]
    require(assets["scanning"] is False and assets["importing"] is False,
            "Godot imports are not idle")


def observe(cli, socket, sessionfile, output, label, command, args):
    require(command in READ_COMMANDS, "Preflight permits read operations only")
    argv = [str(cli), "--socket", socket, "--session-file", str(sessionfile),
            "--json", "execute", "driver.godot." + command,
            "--args-json", json.dumps(args, separators=(",", ":"))]
    save_new(output / (label + ".invocation.json"),
             {"command": "driver.godot." + command, "args": args,
              "read_only_observation": True})
    try:
        child = subprocess.run(argv, capture_output=True, timeout=25)
        raw, err, code = child.stdout, child.stderr, child.returncode
    except subprocess.TimeoutExpired as exc:
        with (output / (label + ".stdout.partial")).open("xb") as stream:
            stream.write(exc.stdout or b"")
        with (output / (label + ".stderr.partial")).open("xb") as stream:
            stream.write(exc.stderr or b"")
        raise PreflightError("Read-only observation timed out: " + label) from exc
    with (output / (label + ".envelope.json")).open("xb") as stream:
        stream.write(raw)
    with (output / (label + ".stderr.log")).open("xb") as stream:
        stream.write(err)
    require(code == 0, "Read-only client failed: " + label)
    envelope = json.loads(raw)
    require(envelope.get("ok") is True and not envelope.get("error"),
            "Read-only broker rejection: " + label)
    execution = envelope["execution"]
    require(execution["backend"] == "driver:godot"
            and execution["policy_decision"] == "allow"
            and execution["provenance"]["source"] == "driver",
            "Unexpected read-only provider route")
    require(envelope.get("command") == "driver.godot." + command
            and isinstance(envelope.get("request_id"), str),
            "Observation command/request identity absent")
    return envelope["data"]


def run_preflight(app, semwright, directory, console_dir, name):
    """Fresh reads are observations, never a durable authorization or a submit."""
    app, semwright = Path(app), Path(semwright)
    directory, console_dir = Path(directory), Path(console_dir)
    output = directory / (
        name + ".preflight-" +
        datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%SZ") +
        "-" + uuid.uuid4().hex[:8]
    )
    output.mkdir(exist_ok=False)
    summary = {
        "schema": 1, "utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "state": "BLOCKED_PREFLIGHT", "passed": False,
        "read_only": True, "native_approval_requests_submitted": 0,
        "approval_input_automated": False, "automatic_retry": False,
        "main_live_projection": "NOT_REOBSERVED_INACTIVE_TAB_REQUIRES_CONSENT",
        "G6": "NOT_COMPLETE", "art_status": "REVISE",
        "human_camera_claimed": False,
    }
    before = None
    try:
        plan = json.loads((directory / (name + ".prepared.json")).read_text())
        config = directory / (name + ".json")
        cfg = json.loads(config.read_text())
        registry = json.loads((directory / "prepared.json").read_text())
        console = json.loads((console_dir / "receipt.json").read_text())
        launch = json.loads((console_dir / "launch.json").read_text())
        require(plan["state"] == "PREPARED_NOT_SUBMITTED", "Plan already submitted")
        require(console["ready_for_human"]
                and console["approval_prompts_submitted"] == 0
                and launch["native_approval_console"], "Native console not prepared")
        require(not any((directory / item).exists()
                        for item in (name, name + ".log", name + ".runner.json",
                                     name + ".submission.json")),
                "Earlier submission exists; reconcile it without replay")
        require(cfg.get("native_operator_approval_required") is True
                and cfg.get("approval_response_must_be_human") is True,
                "Native human approval requirement changed")
        build = semwright / ".local/logs/native-profile-20261028-godot-handoff-proof-build-attempt07/run.json"
        require(digest(build) == plan["build_report_sha256"], "Proof build report changed")
        require(digest(config) == plan["config_sha256"], "Proposal changed")
        require(digest(Path(plan["binary"])) == plan["binary_sha256"], "Proof binary changed")
        require(digest(semwright / SOURCE) == plan["source_sha256"], "Proof source changed")
        require(digest(directory / (name + ".source-before.json")) ==
                plan["source_inventory_sha256"], "Source baseline changed")
        for row in registry["candidate_files"]:
            require(digest(row["path"]) == row["sha256"], "Candidate binary changed")
        for key, expected in launch["configuration_sha256"].items():
            require(digest(registry[key]) == expected, "Broker configuration changed")
        expected_processes = {"broker": console["new_broker"], "editor": console["editor"]}
        for label, expected in expected_processes.items():
            require(expected["pid"] == plan[label + "_pid"], "Prepared process mismatch")
            observed = process_identity(expected["pid"])
            check_process_identity(expected, observed)
            summary[label] = observed
        expected_sources = json.loads((directory / (name + ".source-before.json")).read_text())
        before = inventory(app / "game")
        require(before == expected_sources, "Protected project sources changed")
        require(inventory(Path(plan["recovery_copy"])) == before,
                "Recovery source copy changed or contains an unsafe path")
        baseline = {"sessions": console["sessions"],
                    "scene": json.loads((console_dir / "after-scene.envelope.json").read_text())["data"]}
        cli = Path(registry["paths"]["bin"]) / "semwright.exe"
        sessionfile = Path(registry["paths"]["config"]) / "client-session-operator03.json"
        # crates/protocol/src/lib.rs deliberately does not persist bearer tickets
        # on Windows: connect_persistent authenticates a fresh broker client each
        # time. The pinned Godot editor session/generation is checked separately.
        summary["broker_client_ticket_persistence"] = "DISABLED_BY_WINDOWS_PROTOCOL"
        observations = {}
        for label, command, args in (
            ("sessions", "session.list", {}),
            ("project", "project.inspect", {"session": cfg["editor_session"]}),
            ("scene", "scene.inspect", {"session": cfg["editor_session"]}),
            ("camera", "node.inspect", {"session": cfg["editor_session"], "path": "ProofCamera"}),
            ("editor", "editor.state", {"session": cfg["editor_session"]}),
            ("assets", "assets.status", {"session": cfg["editor_session"]}),
        ):
            observations[label] = observe(cli, registry["socket"], sessionfile,
                                          output, label, command, args)
        validate_live(cfg, baseline, observations)
        # Pin the session again after the observation window. No reconnect or mutation.
        last_sessions = observe(cli, registry["socket"], sessionfile, output,
                                "sessions-after", "session.list", {})
        require(last_sessions == observations["sessions"], "Session changed during preflight")
        for label, expected in expected_processes.items():
            check_process_identity(expected, process_identity(expected["pid"]))
        after = inventory(app / "game")
        require(after == before, "Project sources changed during read-only observation")
        summary.update(
            passed=True, state="READY_FOR_NATIVE_OPERATOR",
            source_files_unchanged=len(before),
            source_inventory_sha256=plan["source_inventory_sha256"],
            fixture_stamp=observations["scene"]["stamp"],
            fixture_projection_preserved=True, fixture_camera_preserved=True,
            unsaved_scenes=observations["editor"]["data"]["unsaved_scenes"],
            sessions=last_sessions, protected_source_files_unchanged=True,
            proof_binary_sha256=plan["binary_sha256"],
            proof_source_sha256=plan["source_sha256"],
            configuration_sha256=plan["config_sha256"],
        )
    except Exception as exc:
        summary["error"] = type(exc).__name__ + ": " + str(exc)
        if before is not None:
            try:
                summary["protected_source_files_unchanged"] = inventory(app / "game") == before
            except Exception as after_error:
                summary["after_inventory_error"] = str(after_error)
    summary["evidence_directory"] = str(output)
    summary["guard_source_sha256"] = digest(Path(__file__))
    save_new(output / "receipt.json", summary)
    return summary


def classify_requests(directory):
    """Conservative, per-request outcomes; missing or uncertain replies stay UNKNOWN."""
    directory = Path(directory)
    outcomes = []
    for request_path in sorted(directory.glob("*.request.json")):
        response_path = request_path.with_name(
            request_path.name[:-len(".request.json")] + ".envelope.json")
        row = {"request_file": request_path.name, "classification": "UNKNOWN"}
        try:
            request = json.loads(request_path.read_text())
            row.update(request_id=request["request_id"], command=request["command"])
            envelope = json.loads(response_path.read_text())
            require(envelope.get("request_id") == request["request_id"]
                    and envelope.get("command") == request["command"],
                    "Request/response identity mismatch")
            error = envelope.get("error")
            execution = envelope.get("execution") or {}
            row.update(ok=envelope.get("ok"), backend=execution.get("backend"),
                       policy_decision=execution.get("policy_decision"))
            mutation = request["command"] in {
                "driver.godot.asset.reimport", "driver.godot.scene.open",
                "driver.godot.scene.save", "driver.godot.camera.configure",
            }
            data = envelope.get("data") or {}
            row["mutation_applied"] = mutation and data.get("applied") is True
            if envelope.get("ok") is True and not error:
                expected_policy = ("allow_after_confirmation" if request["command"] in {
                    "driver.godot.asset.reimport", "driver.godot.scene.open"} else "allow")
                require(execution.get("backend") == "driver:godot"
                        and execution.get("policy_decision") == expected_policy,
                        "Success lacks the expected first-party policy route")
                require(not mutation or row["mutation_applied"], "Mutation was not applied")
                row["classification"] = "KNOWN_SUCCESS"
            elif envelope.get("ok") is False and isinstance(error, dict) \
                    and error.get("outcome_known") is True:
                row["classification"] = (
                    "DENIED" if error.get("code") in
                    ("ConsentRequired", "PermissionDenied", "PolicyDenied", "Denied")
                    else "KNOWN_REJECTION")
                row["error_code"] = error.get("code")
        except Exception as exc:
            row["reason"] = type(exc).__name__ + ": " + str(exc)
        outcomes.append(row)
    return outcomes


def execute_client_once(argv, cwd, env, log_path, timeout=810):
    """Supervise only the owned proof client. Its timeout does NOT cancel the broker."""
    result = {"client_started": False, "exit_code": None, "execution_error": None,
              "client_timeout": False, "broker_operation_cancelled": False}
    child = None
    with Path(log_path).open("xb") as log:
        try:
            child = subprocess.Popen(argv, cwd=cwd, env=env, stdout=log,
                                     stderr=subprocess.STDOUT)
            result["client_started"] = True
            result["client_pid"] = child.pid
            try:
                result["exit_code"] = child.wait(timeout=timeout)
            except subprocess.TimeoutExpired:
                result["client_timeout"] = True
                result["execution_error"] = "Proof client deadline; broker outcome UNKNOWN"
                child.kill()  # This Popen handle owns only the proof client, never the broker/editor.
                result["exit_code"] = child.wait(timeout=10)
        except (Exception, KeyboardInterrupt) as exc:
            result["execution_error"] = type(exc).__name__ + ": " + str(exc)
            if child is not None:
                try:
                    if child.poll() is None:
                        child.kill()  # Only the owned proof client may submit later steps.
                    result["exit_code"] = child.wait(timeout=10)
                    result["proof_client_reaped_after_error"] = True
                except (Exception, KeyboardInterrupt) as cleanup:
                    result["proof_client_reaped_after_error"] = False
                    result["client_reap_error"] = type(cleanup).__name__ + ": " + str(cleanup)
    return result


def aggregate_outcomes(rows, execution, proof_passed):
    if not execution["client_started"]:
        return "NOT_SUBMITTED"
    if execution.get("execution_error") or not rows \
            or any(row["classification"] == "UNKNOWN" for row in rows):
        return "UNKNOWN"
    mutations = [row for row in rows if row.get("command") in {
        "driver.godot.asset.reimport", "driver.godot.scene.open",
        "driver.godot.scene.save", "driver.godot.camera.configure",
    }]
    if proof_passed:
        return "KNOWN"
    if any(row["classification"] == "KNOWN_SUCCESS" and row.get("mutation_applied") is True
           for row in mutations):
        return "PARTIAL_KNOWN"
    if any(row["classification"] == "DENIED" for row in mutations):
        return "DENIED"
    return "KNOWN_STOPPED"
