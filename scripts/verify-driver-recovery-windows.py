#!/usr/bin/env python3
"""Bounded Windows acceptance smoke for Semwright's generic Driver Provider recovery.

Build semwrightd, semwright-sandbox and semwright-driver-fixture first. Run on
Windows with --bin-dir and --work-root, both on a trusted local volume. Uses
an isolated named pipe and owner-only files; never modifies an installed broker.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time
import uuid


def run(args, timeout=16):
    return subprocess.run(args, capture_output=True, text=True,
                          errors="replace", timeout=timeout, check=False)


def private_directory(path):
    path.mkdir(parents=True, exist_ok=False)
    sid_cmd = run(["powershell.exe", "-NoProfile", "-Command",
                   "[Security.Principal.WindowsIdentity]::GetCurrent().User.Value"])
    sid = sid_cmd.stdout.strip()
    if sid_cmd.returncode != 0 or not sid.startswith("S-1-5-"):
        raise RuntimeError("Could not establish Windows owner SID")
    acl = run(["icacls.exe", str(path), "/inheritance:r", "/grant:r",
               f"*{sid}:(OI)(CI)F", "*S-1-5-18:(OI)(CI)F"])
    if acl.returncode:
        raise RuntimeError("Could not harden the private fixture directory")
    return sid


def harden_file(path, sid):
    acl = run(["icacls.exe", str(path), "/inheritance:r", "/grant:r",
               f"*{sid}:F", "*S-1-5-18:F"])
    if acl.returncode:
        raise RuntimeError("Could not protect fixture executable")


def cli_execute(cli, pipe, ticket, command):
    response = run([
        str(cli), "--socket", pipe, "--session-file", str(ticket),
        "--json", "execute", command, "--args-json", "{}"
    ])
    try:
        envelope = json.loads(response.stdout)
    except (ValueError, TypeError):
        raise RuntimeError(
            f"CLI returned no JSON envelope for {command} (exit {response.returncode})"
        ) from None
    return response.returncode, envelope


def doctor(cli, pipe, ticket):
    response = run([str(cli), "--socket", pipe, "--session-file", str(ticket),
                    "--json", "doctor"])
    if response.returncode:
        raise RuntimeError("Read-only broker doctor unavailable")
    data = json.loads(response.stdout)
    if data.get("ok") is not True:
        raise RuntimeError("Doctor did not return a success envelope")
    rows = data["data"]["providers"]["providers"]
    matches = [row for row in rows if row.get("route") == "driver:fixture"]
    if len(matches) != 1:
        raise RuntimeError("Fixture route missing or duplicated")
    return matches[0]


def wait_for_broker(proc):
    for _ in range(12):
        if proc.poll() is not None:
            raise RuntimeError(f"Isolated broker exited during startup: {proc.returncode}")
        time.sleep(0.25)


def stop_owned(proc):
    if proc is not None and proc.poll() is None:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait(timeout=5)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bin-dir", type=Path, required=True)
    parser.add_argument("--cli", type=Path, required=True)
    parser.add_argument("--work-root", type=Path, required=True)
    args = parser.parse_args()
    if os.name != "nt":
        raise RuntimeError("This particular physical fixture exercises Windows isolation")
    binaries = args.bin_dir.resolve(strict=True)
    cli = args.cli.resolve(strict=True)
    root_parent = args.work_root.resolve(strict=True)
    expected = ["semwrightd.exe", "semwright-sandbox.exe",
                "semwright-driver-fixture.exe"]
    for name in expected:
        if not (binaries / name).is_file():
            raise RuntimeError(f"Required build output missing: {name}")
    root = root_parent / ("driver-recovery-" + uuid.uuid4().hex[:12])
    sid = private_directory(root)
    driver = root / "fixture.exe"
    shutil.copyfile(binaries / "semwright-driver-fixture.exe", driver)
    harden_file(driver, sid)
    digest = hashlib.sha256(driver.read_bytes()).hexdigest()
    manifest = {
        "manifest_version": 1, "protocol": 2,
        "id": "fixture", "version": "1.0.0",
        "publisher": "semwright-recovery-conformance",
        "executable": str(driver), "sha256": digest,
        "application": {"desktop_id": "org.semwright.DriverFixture",
                        "process_names": [], "supported_versions": []},
        "transport": "stdio_v1", "network": False,
        "resources": {
            "open_files": 128, "processes": 8, "cpu_seconds": 20,
            "operation_cpu_seconds": 0, "address_space_bytes": 536870912,
            "file_size_bytes": 16777216,
        },
        "interfaces": {
            "dynamic_capabilities": True, "cooperative_cancellation": True,
            "events": True, "progress": True, "artifacts": True,
            "health": True, "native_refs": False, "host_tools": False,
        },
    }
    manifest_path = root / "driver.json"
    manifest_path.write_text(json.dumps(manifest, indent=2), encoding="utf-8")
    config = root / "daemon.toml"
    config.write_text(
        "driver_recovery = true\n"
        "driver_network = false\n"
        "drivers = ['" + str(manifest_path) + "']\n"
        "[policy]\nprofile = 'observe'\nallow = ['driver:fixture']\n",
        encoding="utf-8",
    )
    registry = root / "upstreams.toml"
    registry.write_text("version = 1\nupstreams = []\n", encoding="utf-8")
    pipe = rf"\\.\pipe\semwright-{sid.replace('-', '_')}-recovery-{uuid.uuid4().hex[:8]}"
    ticket = root / "cli.session"
    stderr_path = root / "broker.stderr.log"
    log = stderr_path.open("wb")
    proc = None
    try:
        proc = subprocess.Popen([
            str(binaries / "semwrightd.exe"), "--config", str(config),
            "--socket", pipe, "--data-dir", str(root),
            "--mcp-upstreams", str(registry), "--log-format", "json",
        ], stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=log,
            creationflags=subprocess.CREATE_NO_WINDOW)
        wait_for_broker(proc)
        before = doctor(cli, pipe, ticket)
        if not before.get("connected"):
            raise RuntimeError("Initial owner-pinned fixture is not live")
        code, ping = cli_execute(cli, pipe, ticket, "driver.fixture.ping")
        if code != 0 or ping.get("ok") is not True:
            raise RuntimeError("First live sandboxed fixture ping failed")
        code, crashed = cli_execute(cli, pipe, ticket, "driver.fixture.disconnect")
        if code == 0 or crashed.get("ok") is not False:
            raise RuntimeError("Fixture termination did not cause expected request failure")
        recovered = None
        for delay in (1.8, 1.4, 1.7, 1.7, 1.8):
            time.sleep(delay)
            if proc.poll() is not None:
                raise RuntimeError("Broker died instead of recovering its isolated child")
            try:
                row = doctor(cli, pipe, ticket)
            except RuntimeError:
                continue
            if row.get("connected") and row.get("generation", 0) > before.get("generation", 0):
                recovered = row
                break
        if recovered is None:
            raise RuntimeError("No new live provider generation was published")
        code, ping = cli_execute(cli, pipe, ticket, "driver.fixture.ping")
        if code != 0 or ping.get("ok") is not True:
            raise RuntimeError("Recovered sandboxed fixture failed read-only ping")
        summary = {
            "result": "PASS",
            "initial_generation": before["generation"],
            "recovered_generation": recovered["generation"],
            "disconnect_error_code": crashed.get("error", {}).get("code"),
            "fixture_digest": digest,
            "broker_unchanged": proc.poll() is None,
        }
        (root / "result.json").write_text(json.dumps(summary, indent=2), encoding="utf-8")
        print("PHYSICAL_DRIVER_RECOVERY_PASS")
        print(json.dumps(summary, sort_keys=True))
    finally:
        stop_owned(proc)
        log.close()


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        print(f"PHYSICAL_DRIVER_RECOVERY_FAIL: {error}", file=sys.stderr)
        sys.exit(1)
