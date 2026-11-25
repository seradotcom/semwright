"""Hosted Windows smoke for the genuine closed Blender Host and artifact commands.

Uses disposable owner-scoped data and the binaries built by the current CI job.
It never retries effects, admits a remote worker, or certifies an interactive host.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import runpy
import sys
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


CATALOG_COMMANDS = (
    "artifact.handoff",
    "driver.blender.composition.reopen",
    "driver.blender.composition.inspect",
    "driver.blender.scene.inspect",
    "driver.blender.collection.list",
    "driver.blender.introspect.summary",
    "driver.blender.render.settings",
    "driver.blender.render",
    "driver.blender.export.glb",
)


def registered_catalog_bundle(root, sid, output):
    """Reuse the archived source-bound portable; do not rebuild or substitute it."""
    pins = json.loads((root / "pins.json").read_text(encoding="utf-8-sig"))
    record = json.loads((root / "run.json").read_text(encoding="utf-8-sig"))
    assert set(pins) == {"source", "run", "archive_sha256", "driver_sha256"}
    assert re.fullmatch(r"[0-9a-f]{40}", pins["source"])
    assert re.fullmatch(r"[1-9][0-9]{0,19}", pins["run"])
    assert all(re.fullmatch(r"[0-9a-f]{64}", pins[k])
               for k in ("archive_sha256", "driver_sha256"))
    assert record["headSha"] == pins["source"]
    assert str(record["databaseId"]) == pins["run"]
    assert record["status"] == "completed" and record["conclusion"] == "success"
    archive = root / "portable/semwright-1.0.0-windows-x86_64.zip"
    driver = root / "driver/target/debug/semwright-blender-driver.exe"
    assert sha(archive) == pins["archive_sha256"]
    assert sha(driver) == pins["driver_sha256"]
    manifest = json.loads((root / "driver/verification/windows-blender-native/manifest.json")
                          .read_text(encoding="utf-8"))
    assert manifest["source"] == pins["source"] and manifest["sha256"] == pins["driver_sha256"]
    assert manifest["name"] == driver.name and manifest["windows_blender_version"] == "5.2.1"
    assert manifest["native_host_profile"] == "blender_5_2_1_initial"
    sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "release"))
    from portable_package import safe_extract
    from bundle_contract import verify_checksums
    installation = Path(tempfile.mkdtemp(prefix="catalog-pinned-")).resolve(strict=True)
    bundle = safe_extract(archive, "windows", installation)
    assert bundle.name == "semwright-1.0.0-windows-x86_64"
    payload = verify_checksums(bundle)
    runpy.run_path(str(Path(__file__).with_name("pinned-pipe-peer-smoke.py")))["restrict_bundle"](installation, sid)
    installer = ["pwsh", "-NoProfile", "-NonInteractive", "-File",
                 bundle / "Install-Semwright.ps1", "-InPlace"]
    for args in (installer + ["-Check"], installer):
        result = run(args, timeout=60)
        if result.returncode:
            raise RuntimeError("Archived portable registration refused: " + result.stderr[-800:])
    for relative, expected in payload.items():
        assert sha(bundle / relative) == expected
    receipt = bundle / ".semwright-install-receipt"
    assert receipt.is_file()
    write_json(output / "archived-source.json", {
        "source_run": record, "pins": pins, "driver_manifest": manifest,
        "binary_sha256": {name: sha(bundle / "bin" / name) for name in (
            "semwright.exe", "semwrightd.exe", "semwright-mcp.exe",
            "semwright-inspect.exe", "semwright-sandbox.exe")},
        "registered_installation_receipt_sha256": sha(receipt),
        "payload_entries": len(payload)})
    return bundle / "bin", driver.resolve(strict=True), pins


def catalog_envelope(result, command):
    envelope = result["structuredContent"]
    assert result.get("isError", False) is False
    assert envelope["ok"] is True and envelope["command"] == command
    execution = envelope["execution"]
    assert execution["backend"] == "core" and execution["dry_run"] is False
    assert execution["fallbacks_attempted"] == []
    return envelope


def observe_native_catalog(bins, case, env, output, report):
    """Use the registered MCP's attested Windows pipe connection, not a mock."""
    entry = json.loads((case / "config/mcp-client.json").read_text(
        encoding="utf-8"))["mcpServers"]["semwright"]
    assert Path(entry["command"]) == bins / "semwright-mcp.exe"
    client_class = runpy.run_path(str(Path(__file__).with_name(
        "workspace-smoke.py")))["McpClient"]
    responses = {}
    with (output / "mcp.log").open("x", encoding="utf-8") as stderr:
        client = client_class(entry, stderr, env)
        try:
            def call(label, tool, command, arguments):
                envelope = catalog_envelope(client.request("tools/call", {
                    "name": tool, "arguments": arguments}), command)
                path = output / (label + ".response.json")
                write_json(path, envelope)
                responses[label] = {"request_id": envelope["request_id"], "sha256": sha(path)}
                return envelope["data"]
            before = call("doctor-before", "semwright_doctor", "doctor", {})
            assert before["fake"] is False
            provider_status = before["providers"]
            revision = provider_status["revision"]
            assert any(p["route"] == "driver:blender" and p["connected"] is True
                       for p in provider_status["providers"])
            for index, command in enumerate(CATALOG_COMMANDS):
                data = call(f"descriptor-{index:02d}", "capabilities_describe",
                            "capabilities.describe", {"name": command})
                assert data["revision"] == revision
                assert data["capability"]["name"] == command
                assert isinstance(data["provenance"], dict) and data["provenance"]
                assert isinstance(data["routes"], list)
                assert data["availability_is_authorization"] is False
            after = call("doctor-after", "semwright_doctor", "doctor", {})
            assert after["fake"] is False and after["providers"] == provider_status
        finally:
            client.close()
        assert client.process.poll() is not None
    assert len({row["request_id"] for row in responses.values()}) == len(CATALOG_COMMANDS) + 2
    report.update({"catalog_commands": list(CATALOG_COMMANDS),
                   "catalog_revision_before": revision, "catalog_revision_after": revision,
                   "catalog_responses": responses, "mcp_stopped": True,
                   "transport": "registered Windows MCP with attested pipe peer",
                   "new_native_commands": 0})


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--bin-dir", type=Path)
    parser.add_argument("--blender-root", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--preflight-only", action="store_true")
    parser.add_argument("--catalog-root", type=Path)
    options = parser.parse_args()
    catalog_only = options.catalog_root is not None
    if catalog_only and (options.preflight_only or options.bin_dir is not None):
        parser.error("catalog-only requires archived binaries, not build/preflight mode")
    if os.name != "nt" or os.environ.get("GITHUB_ACTIONS") != "true":
        raise SystemExit("This bounded native case runs only on hosted Windows Actions")
    if not options.preflight_only and (options.blender_root is None
                                       or (not catalog_only and options.bin_dir is None)):
        parser.error("native execution requires --bin-dir and --blender-root")
    source = run(["git", "rev-parse", "HEAD"]).stdout.strip()
    if source != os.environ["GITHUB_SHA"]:
        raise RuntimeError("Native case source differs from CI source")
    output = options.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    harness_source = source
    write_json(output / "preflight.json", {"source": None if catalog_only else source,
               "harness_source": harness_source, "powershell": "pwsh",
               "mode": ("archived-catalog-only" if catalog_only else
                        "ownership-only" if options.preflight_only else "native-artifacts"),
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
    runtime = options.blender_root.resolve(strict=True)
    if catalog_only:
        bins, driver_source, catalog_pins = registered_catalog_bundle(
            options.catalog_root.resolve(strict=True), sid, output)
        source = catalog_pins["source"]
    else:
        bins = options.bin_dir.resolve(strict=True)
        driver_source = bins / "semwright-blender-driver.exe"
    # setup intentionally refuses an incomplete adjacent executable bundle.
    required = ("semwright.exe", "semwrightd.exe", "semwright-mcp.exe",
                "semwright-inspect.exe", "semwright-sandbox.exe",
                )
    missing = [name for name in required if not (bins / name).is_file()]
    if not driver_source.is_file():
        missing.append("semwright-blender-driver.exe")
    if missing:
        write_json(output / "result.json", {
            "source": source, "status": "FAILED_PRE_BROKER",
            "phase": "setup_companions", "missing_binaries": missing,
            "native_started": False, "external_host_accepted": False, "release": False})
        raise RuntimeError("Native case missing setup companions: " + ", ".join(missing))
    protect(runtime, sid)
    (workspace / "reserved.png").write_bytes(b"existing-owner-artifact")
    driver = case / "semwright-blender-driver.exe"
    shutil.copyfile(driver_source, driver)
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
    for name in ("GH_TOKEN", "GITHUB_TOKEN"):
        env.pop(name, None)
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
        '[policy]\nprofile = "observe"\nallow = ["driver:blender"]\n'
        + ("confirm_mutations = true\n" if catalog_only else "confirm_mutations = false\n")
        +
        f'[[policy.filesystem]]\nname = "workspace"\npath = \'{workspace}\'\nread = true\nwrite = true\n'
        f'[[policy.filesystem]]\nname = "blender-runtime"\npath = \'{runtime}\'\nread = true\nwrite = false\n'
        f'[[policy.filesystem]]\nname = "blender-executable"\npath = \'{blender}\'\nread = true\nwrite = false\n',
        encoding="utf-8", newline="\n")
    report = {"status": "RUNNING", "source": source, "harness_source": harness_source,
              "fake": None, "steps": [],
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

        if catalog_only:
            observe_native_catalog(bins, case, env, output, report)
            report["status"] = "PASS_ARCHIVED_NATIVE_CATALOG_NO_EFFECT"
        else:
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
        if catalog_only:
            events = []
            for line in log_path.read_text(encoding="utf-8", errors="replace").splitlines():
                try:
                    events.append(json.loads(line))
                except ValueError:
                    pass
            native_started = any(event.get("phase") in (
                "blender_5_2_1_initial_requested", "native_launch_finished",
                "native_blender_ready") for event in events)
            report["native_started"] = native_started
            report["archived_ci_run"] = catalog_pins["run"]
            report["archive_sha256"] = catalog_pins["archive_sha256"]
            if (native_started or report["steps"] or records
                    or {path.name for path in workspace.iterdir()} != {"reserved.png"}):
                report["status"] = "NOT_ACCEPTED_CATALOG_EFFECT"
        report["existing_target_preserved"] = (workspace / "reserved.png").read_bytes() == b"existing-owner-artifact"
        report["remaining_workspace_files"] = sorted(path.name for path in workspace.iterdir())
        write_json(output / "result.json", report)
        print(json.dumps(report, sort_keys=True), flush=True)
    expected = ("PASS_ARCHIVED_NATIVE_CATALOG_NO_EFFECT" if catalog_only
                else "PASS_NATIVE_GLB_NEW_ONLY_PNG")
    if report["status"] != expected or not report["existing_target_preserved"]:
        raise RuntimeError(report["status"])


if __name__ == "__main__":
    main()
