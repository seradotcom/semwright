"""Native Windows broker restart continuity for a single disposable Project Graph.

Runs in hosted CI using freshly built binaries; no fake backend, client replay
or third-party agent-host acceptance is implied.
"""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import secrets
import subprocess
import time


def result_json(argv: list[str], env: dict[str, str], timeout: int = 10):
    proc = subprocess.run(
        argv,
        env=env,
        capture_output=True,
        text=True,
        encoding="utf-8",
        timeout=timeout,
    )
    try:
        return proc.returncode, json.loads(proc.stdout)
    except json.JSONDecodeError as error:
        raise RuntimeError("Native CLI returned non-JSON response") from error


def revision_key(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"))


class LiveBroker:
    def __init__(self, *, binary: Path, config: Path, env: dict, output: Path, cli: Path, name: str):
        self.bin = binary
        self.config = config
        self.env = env
        self.cli = cli
        self.pipe = r"\\.\pipe\semwright-project-restart-" + secrets.token_hex(9)
        self.ticket = output / f"{name}.session"
        self.log = (output / f"{name}.broker.log").open("x", encoding="utf-8")
        self.process = None

    def launch(self):
        self.process = subprocess.Popen(
            [str(self.bin), "--config", str(self.config), "--socket", self.pipe],
            env=self.env,
            stdin=subprocess.DEVNULL,
            stdout=self.log,
            stderr=self.log,
        )
        deadline = time.monotonic() + 16
        while time.monotonic() < deadline:
            if self.process.poll() is not None:
                raise RuntimeError("Native broker exited before readiness")
            try:
                code, report = result_json(
                    [str(self.cli), "--socket", self.pipe, "--session-file", str(self.ticket),
                     "--json", "doctor"],
                    self.env,
                    timeout=8,
                )
                if code == 0 and report.get("ok") and report.get("data", {}).get("fake") is False:
                    return
            except subprocess.TimeoutExpired:
                pass
            time.sleep(0.1)
        raise TimeoutError("Native broker never became ready")

    def execute(self, command: str, args: dict):
        _, report = result_json(
            [str(self.cli), "--socket", self.pipe, "--session-file", str(self.ticket),
             "--json", "execute", command, "--args-json", json.dumps(args)],
            self.env,
        )
        assert report.get("command") == command, report
        return report

    def require(self, command: str, args: dict):
        response = self.execute(command, args)
        assert response["ok"] is True, response
        return response["data"]

    def stop(self):
        try:
            if self.process is not None and self.process.poll() is None:
                self.process.terminate()
                try:
                    self.process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    self.process.kill()
                    self.process.wait(timeout=5)
        finally:
            self.log.close()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--bin-dir", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if os.name != "nt":
        raise RuntimeError("This native process test requires Windows")
    binary = args.bin_dir.resolve()
    output = args.output.resolve()
    output.mkdir(exist_ok=False, parents=True)
    cli = binary / "semwright.exe"
    daemon = binary / "semwrightd.exe"
    assert cli.is_file() and daemon.is_file()
    workspace = output / "workspace"
    workspace.mkdir()
    document = workspace / "asset.bin"
    document.write_bytes(b"registered")
    private_data = output / "private-data"
    config_dir = output / "config"
    env = dict(os.environ, SEMWRIGHT_DATA_DIR=str(private_data))
    first = second = None
    report = {
        "native_broker": True,
        "fake": False,
        "first_project_registered": False,
        "first_broker_stopped": False,
        "same_owner_reopened": False,
        "offline_in_place_edit_reconciled": False,
        "other_file_not_adopted": False,
        "external_host_accepted": False,
    }
    try:
        code, setup = result_json(
            [str(cli), "--socket", r"\\.\pipe\semwright-project-setup-" + secrets.token_hex(7),
             "--json", "setup", "--config-dir", str(config_dir)],
            env,
        )
        assert code == 0 and setup["authority_changed"] is False
        assert setup["service_started"] is False
        config = Path(setup["config"]["path"])
        assert config == config_dir / "daemon.toml"
        config.write_text(
            '[policy]\nprofile = "observe"\n'
            'allow = ["project.manage"]\n'
            'confirm_mutations = false\n'
            '[[policy.filesystem]]\nname = "workspace"\n'
            f"path = '{workspace}'\nread = true\nwrite = false\n",
            encoding="utf-8",
        )
        first = LiveBroker(binary=daemon, config=config, env=env, output=output,
                           cli=cli, name="first")
        first.launch()
        project = first.require("project.create", {"root": "workspace"})["project"]
        registered = first.require("project.asset.register", {
            "root": "workspace", "project": project, "label": "asset",
            "resource_type": "fixture", "path": "asset.bin", "max_bytes": 1024,
        })
        asset = registered["result"]["asset"]["id"]
        keys = {"root": "workspace", "project": project, "asset": asset}
        revisions = first.require("project.revisions", keys)["result"]["revisions"]
        assert len(revisions) == 1, revisions
        initial = {revision_key(row["pin"]) for row in revisions}
        report["first_project_registered"] = True
        first.stop()
        first = None
        report["first_broker_stopped"] = True

        # A separate owner-side edit takes place while the broker is offline.
        document.write_bytes(b"edited-offline")
        second = LiveBroker(binary=daemon, config=config, env=env, output=output,
                            cli=cli, name="second")
        second.launch()
        opened = second.require("project.query", {
            "root": "workspace", "project": project,
        })
        assert opened["project"] == project
        report["same_owner_reopened"] = True
        second.require("project.asset.reconcile", keys)
        current = second.require("project.revisions", keys)["result"]["revisions"]
        current_pins = {revision_key(row["pin"]) for row in current}
        assert len(current) == 2 and initial < current_pins, current
        report["offline_in_place_edit_reconciled"] = True

        other = workspace / "other.bin"
        other.write_bytes(b"unrelated-instance")
        os.replace(other, document)
        attempted = second.execute("project.asset.reconcile", keys)
        if not attempted["ok"]:
            assert attempted["error"]["code"] == "Conflict", attempted
        after = second.require("project.revisions", keys)["result"]["revisions"]
        assert {revision_key(row["pin"]) for row in after} == current_pins
        assert document.read_bytes() == b"unrelated-instance"
        report["other_file_not_adopted"] = True
    finally:
        if first is not None:
            first.stop()
        if second is not None:
            second.stop()
        (output / "result.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    assert all(report[k] for k in (
        "first_project_registered",
        "first_broker_stopped",
        "same_owner_reopened",
        "offline_in_place_edit_reconciled",
        "other_file_not_adopted",
    )), report
    print(json.dumps(report))


if __name__ == "__main__":
    main()
