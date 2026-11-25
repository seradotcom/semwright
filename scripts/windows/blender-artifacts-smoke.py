"""Hosted Windows smoke for the genuine closed Blender Host and artifact commands.

Uses disposable owner-scoped data and the binaries built by the current CI job.
It never retries effects, admits a remote worker, or certifies an interactive host.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import secrets
import shutil
import struct
import subprocess
import tempfile
import time


def sha(path):
    digest = hashlib.sha256()
    with Path(path).open("rb") as stream:
        for block in iter(lambda: stream.read(1048576), b""):
            digest.update(block)
    return digest.hexdigest()


def write_json(path, value):
    with path.open("x", encoding="utf-8", newline="\n") as stream:
        json.dump(value, stream, indent=2, sort_keys=True)
        stream.write("\n")


def run(argv, *, env=None, timeout=30):
    return subprocess.run([str(value) for value in argv], env=env, timeout=timeout,
                          capture_output=True, text=True, encoding="utf-8", errors="replace")


def protect(path, sid):
    proc = run(["icacls", path, "/inheritance:r", "/grant:r",
                f"*{sid}:(OI)(CI)F", "*S-1-5-18:(OI)(CI)F", "*S-1-5-32-544:(OI)(CI)F"])
    if proc.returncode:
        raise RuntimeError("Cannot restrict disposable native test directory")



def workspace_security(path, sid):
    quoted = str(path).replace("'", "''")
    script = (
        "$acl=Get-Acl -LiteralPath '" + quoted + "';"
        "$sid='" + sid + "';"
        "$full=[System.Security.AccessControl.FileSystemRights]::FullControl;"
        "$rules=@($acl.Access | Where-Object {"
        "$_.IdentityReference.Translate([System.Security.Principal.SecurityIdentifier]).Value -eq $sid "
        "-and $_.AccessControlType -eq 'Allow' -and -not $_.IsInherited "
        "-and ($_.FileSystemRights -band $full) -eq $full "
        "-and ([int]$_.InheritanceFlags -band 3) -eq 3 "
        "-and [int]$_.PropagationFlags -eq 0 });"
        "@{owner_matches_user=($acl.GetOwner([System.Security.Principal.SecurityIdentifier]).Value -eq $sid);"
        "protected=$acl.AreAccessRulesProtected;explicit_inheritable_full=($rules.Count -gt 0)}"
        " | ConvertTo-Json -Compress")
    result = run(["pwsh", "-NoProfile", "-NonInteractive", "-Command", script])
    if result.returncode:
        raise RuntimeError("Cannot inspect new CI workspace ownership: " + result.stderr[-1600:])
    return json.loads(result.stdout)


def cli_json(cli, common, args, env, timeout=30):
    proc = run([cli, *common, *args], env=env, timeout=timeout)
    try:
        return proc.returncode, json.loads(proc.stdout)
    except ValueError as error:
        raise RuntimeError("Native CLI returned a non-JSON response: " + proc.stderr[-400:]) from error


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--bin-dir", type=Path)
    parser.add_argument("--blender-root", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--preflight-only", action="store_true")
    options = parser.parse_args()
    if os.name != "nt" or os.environ.get("GITHUB_ACTIONS") != "true":
        raise SystemExit("This bounded native case runs only on hosted Windows Actions")
    if not options.preflight_only and (options.bin_dir is None or options.blender_root is None):
        parser.error("native execution requires --bin-dir and --blender-root")
    source = run(["git", "rev-parse", "HEAD"]).stdout.strip()
    if source != os.environ["GITHUB_SHA"]:
        raise RuntimeError("Native case source differs from CI source")
    output = options.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    write_json(output / "preflight.json", {"source": source, "powershell": "pwsh",
               "mode": "ownership-only" if options.preflight_only else "native-artifacts",
               "native_started": False})
    sid = run(["pwsh", "-NoProfile", "-NonInteractive", "-Command",
               "[Security.Principal.WindowsIdentity]::GetCurrent().User.Value"]).stdout.strip()
    if not sid.startswith("S-1-5-21-"):
        raise RuntimeError("Runner owner SID unavailable")
    # Preserve the runner's private temp ancestry while leaving headroom for
    # Blender's deepest Python modules and the Host's unique staging directory.
    case = Path(tempfile.mkdtemp(prefix="b")).resolve()
    protect(case, sid)
    data_root = case / "d"
    # Keep the private broker state under the already qualified OS temp root.
    # The owner-authorized workspace is a distinct disposable directory on
    # the hosted runner volume, not nested below the private state case.
    runner_temp = Path(os.environ["RUNNER_TEMP"]).resolve(strict=True)
    workspace = Path(tempfile.mkdtemp(prefix="w", dir=runner_temp)).resolve(strict=True)
    before_security = workspace_security(workspace, sid)
    # An elevated hosted runner can create Administrator-owned children. Only
    # this newly created fixture is assigned to the active runner; Core's
    # current-user ownership requirement remains unchanged.
    owner = run(["icacls", workspace, "/setowner", "*" + sid])
    if owner.returncode:
        raise RuntimeError("Cannot assign the new fixture to the active runner")
    protect(workspace, sid)
    after_security = workspace_security(workspace, sid)
    write_json(output / "workspace-security.json",
               {"before": before_security, "after": after_security,
                "private_data_root_utf16_units": len(str(data_root).encode("utf-16-le")) // 2,
                "workspace_root_kind": "separate_runner_temp",
                "workspace_drive_differs_from_data": workspace.drive != data_root.drive})
    if not all(after_security.values()):
        raise RuntimeError("New fixture does not satisfy native workspace ownership")
    if options.preflight_only:
        shutil.rmtree(workspace)
        shutil.rmtree(case)
        write_json(output / "result.json",
                   {"source": source, "status": "PASS_NATIVE_FIXTURE_OWNERSHIP",
                    "native_started": False, "fixture_removed": True})
        print("Native fixture ownership preflight passed; no broker or Blender started", flush=True)
        return
    bins, runtime = options.bin_dir.resolve(), options.blender_root.resolve()
    # setup intentionally refuses an incomplete adjacent executable bundle.
    required = ("semwright.exe", "semwrightd.exe", "semwright-mcp.exe",
                "semwright-inspect.exe", "semwright-sandbox.exe",
                "semwright-blender-driver.exe")
    missing = [name for name in required if not (bins / name).is_file()]
    if missing:
        write_json(output / "result.json", {
            "source": source, "status": "FAILED_PRE_BROKER",
            "phase": "setup_companions", "missing_binaries": missing,
            "native_started": False, "external_host_accepted": False, "release": False})
        raise RuntimeError("Native case missing setup companions: " + ", ".join(missing))
    protect(runtime, sid)
    (workspace / "reserved.png").write_bytes(b"existing-owner-artifact")
    driver = case / "semwright-blender-driver.exe"
    shutil.copyfile(bins / driver.name, driver)
    blender = runtime / "blender.exe"
    manifest = {
        "manifest_version": 1, "protocol": 8, "id": "blender", "version": "1.0.0",
        "publisher": "semwright", "executable": str(driver), "sha256": sha(driver),
        "application": {"desktop_id": "org.blender.Blender", "process_names": ["blender"],
                        "supported_versions": ["5.2.1"]},
        "transport": "stdio_v1",
        "mounts": [{"root": "workspace", "read_only": False, "execute": False},
                   {"root": "blender-runtime", "read_only": True, "execute": True}],
        "system_config": [], "secrets": [],
        "tools": [{"root": "blender-executable", "name": "blender", "sha256": sha(blender),
                   "mounts": ["workspace", "blender-runtime"], "system_config": [],
                   "dependencies": [], "nvidia_gpu": False,
                   "windows_native_profile": "blender_5_2_1_initial"}],
        "network": False, "nvidia_gpu": False, "loopback_port": None,
        "resources": {"open_files": 256, "processes": 8, "cpu_seconds": 300,
                      "operation_cpu_seconds": 0, "address_space_bytes": 4294967296,
                      "file_size_bytes": 1073741824},
        "request_timeout_ms": 300000,
        "interfaces": {"dynamic_capabilities": False, "cooperative_cancellation": True,
                       "events": False, "health": True, "progress": True, "artifacts": False,
                       "native_refs": False, "host_tools": True},
    }
    manifest_path = case / "blender.driver.json"
    write_json(manifest_path, manifest)
    env = dict(os.environ)
    env["SEMWRIGHT_DATA_DIR"] = str(data_root)
    env["PYTHONDONTWRITEBYTECODE"] = "1"
    env.pop("SEMWRIGHT_MCP_UPSTREAMS", None)
    pipe = r"\\.\pipe\semwright-native-artifacts-" + secrets.token_hex(12)
    cli = bins / "semwright.exe"
    common = ["--socket", pipe, "--session-file", str(case / "client.session"), "--json"]
    code, setup = cli_json(cli, common, ["setup", "--config-dir", str(case / "config")], env)
    if code or setup.get("authority_changed") is not False or setup.get("service_started") is not False:
        # Preserve bounded first-party status, not the setup document, ticket or paths.
        error = setup.get("error")
        error_code = error.get("code") if isinstance(error, dict) else None
        if not isinstance(error_code, str) or len(error_code) > 80 or not error_code.isascii() or not error_code.isalnum():
            error_code = None
        write_json(output / "result.json", {
            "source": source, "status": "FAILED_PRE_BROKER", "phase": "owner_setup",
            "exit_code": code, "error_code": error_code,
            "authority_unchanged": setup.get("authority_changed") is False,
            "service_not_started": setup.get("service_started") is False,
            "native_started": False, "external_host_accepted": False, "release": False})
        raise RuntimeError("Native case owner setup failed")
    config = case / "config/native.toml"
    paths = (manifest_path, workspace, runtime, blender)
    if any("'" in str(path) for path in paths):
        raise RuntimeError("Native test path is not TOML literal-safe")
    config.write_text(
        f"drivers = ['{manifest_path}']\ndriver_windows_blender52_initial = true\n"
        "driver_network = false\ndriver_nvidia_gpu = false\ndriver_recovery = false\n"
        '[policy]\nprofile = "observe"\nallow = ["driver:blender"]\nconfirm_mutations = false\n'
        f'[[policy.filesystem]]\nname = "workspace"\npath = \'{workspace}\'\nread = true\nwrite = true\n'
        f'[[policy.filesystem]]\nname = "blender-runtime"\npath = \'{runtime}\'\nread = true\nwrite = false\n'
        f'[[policy.filesystem]]\nname = "blender-executable"\npath = \'{blender}\'\nread = true\nwrite = false\n',
        encoding="utf-8", newline="\n")
    report = {"status": "RUNNING", "source": source, "fake": None, "steps": [],
              "native_profile": "Blender 5.2.1 Windows", "blender_sha256": sha(blender),
              "driver_sha256": sha(driver), "external_host_accepted": False, "release": False}
    broker = None
    log_path = output / "broker.log"
    log = log_path.open("x", encoding="utf-8")
    try:
        broker = subprocess.Popen([str(bins / "semwrightd.exe"), "--config", str(config),
                                   "--socket", pipe], env=env, stdin=subprocess.DEVNULL,
                                  stdout=log, stderr=log)
        report["broker_pid"] = broker.pid
        deadline = time.monotonic() + 220
        while time.monotonic() < deadline:
            if broker.poll() is not None:
                raise RuntimeError("Native broker exited before readiness")
            try:
                code, doctor = cli_json(cli, common, ["doctor"], env, 8)
                if code == 0 and doctor.get("ok") and doctor.get("data", {}).get("fake") is False:
                    report["fake"] = False
                    break
            except (subprocess.TimeoutExpired, RuntimeError):
                pass
            time.sleep(0.25)
        else:
            raise TimeoutError("Native broker did not become ready")

        def execute(label, command, args, expected_error=None):
            code, envelope = cli_json(cli, common,
                ["execute", "driver.blender." + command, "--args-json", json.dumps(args)], env, 90)
            write_json(output / (label + ".response.json"), envelope)
            report["steps"].append({"command": command, "request_id": envelope.get("request_id"),
                                    "ok": envelope.get("ok")})
            if envelope.get("execution", {}).get("fallbacks_attempted"):
                raise RuntimeError("Unexpected native fallback")
            if expected_error:
                if code == 0 or envelope.get("ok") is not False or envelope["error"]["code"] != expected_error:
                    raise RuntimeError("Native negative returned an unexpected result")
                return envelope["error"]
            if code or envelope.get("ok") is not True:
                raise RuntimeError(command + " failed without replay: " + str(envelope.get("error")))
            return envelope["data"]

        summary = execute("summary", "introspect.summary", {})
        assert summary["version"] == [5, 2, 1] and summary["arbitrary_python"] is False
        execute("create", "object.create", {"name": "NativeArtifactCube", "primitive": "cube"})
        execute("collection", "collection.create", {"name": "NativeArtifactCollection"})
        execute("link", "collection.link", {"object": "NativeArtifactCube", "collection": "NativeArtifactCollection"})
        exported = execute("export", "export.glb",
                           {"collection": "NativeArtifactCollection", "path": "model.glb", "animations": False})
        model = workspace / "model.glb"
        data = model.read_bytes()
        assert struct.unpack("<4sII", data[:12]) == (b"glTF", 2, len(data))
        assert exported["bytes"] == len(data) and exported["sha256"] == sha(model)
        execute("settings", "render.settings",
                {"width": 32, "height": 32, "engine": "CYCLES", "samples": 1, "device": "CPU"})
        rendered = execute("render", "render", {"path": "preview.png", "overwrite": False})
        assert rendered == {"changed": True, "path": "preview.png", "format": "png"}
        preview = workspace / "preview.png"
        data = preview.read_bytes()
        assert data[:8] == b"\x89PNG\r\n\x1a\n" and struct.unpack(">II", data[16:24]) == (32, 32)
        refusal = execute("existing", "render", {"path": "reserved.png", "overwrite": False}, "Conflict")
        assert refusal["outcome_known"] is False
        assert (workspace / "reserved.png").read_bytes() == b"existing-owner-artifact"
        assert not list(workspace.glob(".semwright-render-*")) and not list(workspace.glob(".semwright-export-*"))
        artifacts = output / "artifacts"
        artifacts.mkdir()
        for path in (model, preview):
            shutil.copyfile(path, artifacts / path.name)
        report["artifacts"] = {path.name: {"bytes": path.stat().st_size, "sha256": sha(path)}
                               for path in (model, preview)}
        events = []
        for line in log_path.read_text(encoding="utf-8", errors="replace").splitlines():
            try:
                events.append(json.loads(line))
            except ValueError:
                pass
        ready = [event["pid"] for event in events if event.get("phase") == "native_blender_ready"]
        attested = {event["pid"] for event in events
                    if event.get("phase") == "initial_process_attested_before_resume"
                    and event.get("exact_job") is True}
        assert len(ready) == 1 and ready[0] in attested
        report["native_pid"] = ready[0]
        report["status"] = "PASS_NATIVE_GLB_NEW_ONLY_PNG"
    except BaseException as error:
        report["status"] = "FAILED_OR_UNKNOWN_NO_REPLAY"
        report["error"] = str(error)[:1200]
        raise
    finally:
        if broker is not None and broker.poll() is None:
            broker.terminate()
            try:
                broker.wait(timeout=10)
            except subprocess.TimeoutExpired:
                broker.kill()
                broker.wait(timeout=5)
        log.close()
        report["broker_stopped"] = broker is not None and broker.poll() is not None
        pids = []
        for line in log_path.read_text(encoding="utf-8", errors="replace").splitlines():
            try:
                event = json.loads(line)
            except ValueError:
                continue
            if event.get("phase") == "initial_process_attested_before_resume":
                pids.append(int(event["pid"]))
        remaining = run(["pwsh", "-NoProfile", "-NonInteractive", "-Command",
                         "$ids=@(" + ",".join(map(str, pids)) +
                         "); foreach($idValue in $ids) { Get-Process -Id $idValue "
                         "-ErrorAction SilentlyContinue | Select-Object -ExpandProperty Id }; exit 0"])
        report["native_processes_stopped"] = remaining.returncode == 0 and not remaining.stdout.strip()
        if not report["broker_stopped"] or not report["native_processes_stopped"]:
            report["status"] = "NOT_ACCEPTED_PROCESS_CLEANUP"
        records = sorted(workspace.glob(".semwright-native-failure-*.json"))
        if records:
            diagnostics = output / "diagnostics"
            diagnostics.mkdir()
            for path in records[:8]:
                if path.stat().st_size <= 8192:
                    shutil.copyfile(path, diagnostics / path.name.removeprefix("."))
            report["local_diagnostics"] = len(list(diagnostics.iterdir()))
        report["existing_target_preserved"] = (workspace / "reserved.png").read_bytes() == b"existing-owner-artifact"
        report["remaining_workspace_files"] = sorted(path.name for path in workspace.iterdir())
        write_json(output / "result.json", report)
        print(json.dumps(report, sort_keys=True), flush=True)
    if report["status"] != "PASS_NATIVE_GLB_NEW_ONLY_PNG":
        raise RuntimeError(report["status"])


if __name__ == "__main__":
    main()
