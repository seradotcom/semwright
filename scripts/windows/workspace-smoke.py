"""Windows real-broker/MCP workspace smoke; uses only disposable project data.

Run with binaries built from this source. This is protocol/backend evidence,
not acceptance in a third-party agent host or an interactive desktop.
"""
from __future__ import annotations

import argparse
import difflib
import hashlib
import json
import os
from pathlib import Path
import queue
import subprocess
import tempfile
import threading
import time
import uuid


class McpClient:
    def __init__(self, entry, stderr, environment):
        self.process = subprocess.Popen(
            [entry["command"], *entry.get("args", [])],
            env=dict(environment, **entry.get("env", {})),
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=stderr,
            text=True, encoding="utf-8",
        )
        self.messages = queue.Queue()
        self.sequence = 0
        self.reader = threading.Thread(target=self._read, daemon=True)
        self.reader.start()
        try:
            self.request("initialize", {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "semwright-workspace-smoke", "version": "1"},
            })
            self.send({"jsonrpc": "2.0", "method": "notifications/initialized"})
        except BaseException:
            self.close()
            raise

    def _read(self):
        try:
            for line in self.process.stdout:
                self.messages.put(json.loads(line))
        except Exception as error:
            self.messages.put(error)
        finally:
            self.messages.put(EOFError("MCP process closed stdout"))

    def send(self, message):
        self.process.stdin.write(json.dumps(message) + "\n")
        self.process.stdin.flush()

    def request(self, method, params):
        self.sequence += 1
        request_id = self.sequence
        self.send({"jsonrpc": "2.0", "id": request_id, "method": method, "params": params})
        deadline = time.monotonic() + 20
        while True:
            message = self.messages.get(timeout=max(0.01, deadline - time.monotonic()))
            if isinstance(message, Exception):
                raise message
            if message.get("method") == "ping" and "id" in message:
                self.send({"jsonrpc": "2.0", "id": message["id"], "result": {}})
            elif message.get("id") == request_id and "method" not in message:
                if "error" in message:
                    raise AssertionError(message["error"])
                return message["result"]
            if time.monotonic() >= deadline:
                raise TimeoutError(f"MCP request timed out: {method}")

    def execute(self, command, args):
        result = self.request("tools/call", {
            "name": "semwright_execute",
            "arguments": {"command": command, "args": args},
        })
        envelope = result["structuredContent"]
        assert result.get("isError", False) == (not envelope["ok"]), result
        return envelope

    def close(self):
        if self.process.poll() is None:
            self.process.stdin.close()
            try:
                self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.process.terminate()
                self.process.wait(timeout=5)
        self.reader.join(timeout=2)
        self.process.stdout.close()


def check_ok(envelope):
    assert envelope["ok"], envelope
    assert envelope["execution"]["backend"] == "filesystem", envelope
    assert not envelope["execution"]["dry_run"], envelope
    return envelope["data"]


def check_denied(envelope):
    assert not envelope["ok"], envelope
    assert envelope["error"]["outcome_known"], envelope
    assert envelope["error"]["code"] in {
        "PolicyDenied", "PermissionDenied", "InvalidArgument",
    }, envelope


def run(binary_dir: Path, output: Path):
    if os.name != "nt":
        raise RuntimeError("This smoke requires native Windows binaries")
    binary_dir = binary_dir.resolve()
    output = output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    for name in ("semwright", "semwrightd", "semwright-mcp"):
        if not (binary_dir / f"{name}.exe").is_file():
            raise FileNotFoundError(name)
    with tempfile.TemporaryDirectory(prefix="workspace-smoke-", dir=output) as temporary:
        base = Path(temporary)
        workspace = base / "workspace"
        outside = base / "outside"
        artifact_source = base / "artifact-source"
        artifact_destination = base / "artifact-destination"
        workspace.mkdir()
        outside.mkdir()
        artifact_source.mkdir()
        artifact_destination.mkdir()
        # The transferable binary has no text encoding or executable meaning.
        handoff_payload = b"\x00\xff\x01scoped-new-only"
        (artifact_source / "source.bin").write_bytes(handoff_payload)
        sentinel = outside / "secret.txt"
        sentinel.write_text("outside must stay unchanged\n", encoding="utf-8")
        data = workspace / "settings.json"
        before = '{"enabled": false}\n'
        data.write_bytes(before.encode("utf-8"))
        config_dir = base / "owner-config"
        config = config_dir / "daemon.toml"
        data_root = base / "private-data"
        environment = dict(os.environ, SEMWRIGHT_DATA_DIR=str(data_root))
        ticket = base / "client.session"
        endpoint = "\\\\.\\pipe\\semwright-workspace-" + uuid.uuid4().hex
        setup_command = [
            str(binary_dir / "semwright.exe"), "--socket", endpoint,
            "--session-file", str(ticket), "--json", "setup",
            "--config-dir", str(config_dir),
        ]
        dry = subprocess.run(
            setup_command + ["--dry-run"], env=environment,
            capture_output=True, text=True, encoding="utf-8", check=True, timeout=15,
        )
        dry_report = json.loads(dry.stdout)
        assert dry_report["setup"] == "dry-run"
        assert not config_dir.exists() and not data_root.exists()
        setup = subprocess.run(
            setup_command, env=environment, capture_output=True, text=True,
            encoding="utf-8", check=True, timeout=15,
        )
        setup_report = json.loads(setup.stdout)
        assert setup_report["authority_changed"] is False
        assert setup_report["service_started"] is False
        assert Path(setup_report["config"]["path"]) == config
        snippet_path = config_dir / "mcp-client.json"
        snippet_bytes = snippet_path.read_bytes()
        snippet = json.loads(snippet_bytes)["mcpServers"]["semwright"]
        assert Path(snippet["command"]) == binary_dir / "semwright-mcp.exe"
        assert snippet["args"] == ["--socket", endpoint, "--session-file", str(ticket)]
        assert snippet["env"] == {"SEMWRIGHT_DATA_DIR": str(data_root)}
        again = subprocess.run(
            setup_command, env=environment, capture_output=True, text=True,
            encoding="utf-8", check=True, timeout=15,
        )
        assert json.loads(again.stdout)["config"]["status"] == "kept-existing"
        assert snippet_path.read_bytes() == snippet_bytes
        assert not data_root.exists(), "Setup must not start or initialize the runtime"

        # Owner CLI prepares a NEW reviewable workspace policy, not a live grant.
        source_config = config.read_bytes()
        workspace_command = [
            str(binary_dir / "semwright.exe"), "--json", "workspace",
            "--name", "workspace", "--path", str(workspace), "--write",
            "--config-dir", str(config_dir),
        ]
        planned = subprocess.run(
            workspace_command + ["--dry-run"], env=environment,
            capture_output=True, text=True, encoding="utf-8", check=True, timeout=15,
        )
        assert json.loads(planned.stdout)["status"] == "would-create"
        candidate = config_dir / "daemon-workspace.toml"
        assert not candidate.exists()
        prepared = subprocess.run(
            workspace_command, env=environment,
            capture_output=True, text=True, encoding="utf-8", check=True, timeout=15,
        )
        prepared_report = json.loads(prepared.stdout)
        assert prepared_report["status"] == "created"
        assert prepared_report["authority_granted"] is False
        assert prepared_report["broker_started"] is False
        assert config.read_bytes() == source_config
        import tomllib
        candidate_policy = tomllib.loads(candidate.read_text(encoding="utf-8"))["policy"]
        assert candidate_policy["filesystem"] == [{
            "name": "workspace", "path": str(workspace), "read": True, "write": True,
        }]
        candidate_bytes = candidate.read_bytes()
        duplicate = subprocess.run(
            workspace_command, env=environment,
            capture_output=True, text=True, encoding="utf-8", timeout=15,
        )
        assert duplicate.returncode != 0
        assert candidate.read_bytes() == candidate_bytes
        assert config.read_bytes() == source_config
        assert not data_root.exists(), "Workspace preparation must not initialize broker state"
        junction = workspace / "escape"
        subprocess.run(
            ["cmd", "/C", "mklink", "/J", str(junction), str(outside)],
            check=True, capture_output=True, timeout=10,
        )
        daemon = None
        client = None

        def write_config(writable):
            config.write_text(
                '[policy]\nprofile = "observe"\n'
                '[[policy.filesystem]]\nname = "workspace"\n'
                f"path = '{workspace}'\nread = true\nwrite = {str(writable).lower()}\n"
                '[[policy.filesystem]]\nname = "handoff-source"\n'
                f"path = '{artifact_source}'\nread = true\nwrite = false\n"
                '[[policy.filesystem]]\nname = "handoff-destination"\n'
                f"path = '{artifact_destination}'\nread = false\nwrite = true\n",
                encoding="utf-8",
            )

        with (output / "daemon.log").open("w", encoding="utf-8") as daemon_log, (
            output / "mcp.log"
        ).open("w", encoding="utf-8") as mcp_log:
            def start_broker():
                process = subprocess.Popen(
                    setup_report["next"]["broker"], env=environment,
                    stdin=subprocess.DEVNULL, stdout=daemon_log, stderr=daemon_log,
                )
                deadline = time.monotonic() + 20
                try:
                    while time.monotonic() < deadline:
                        if process.poll() is not None:
                            raise RuntimeError("Broker exited; inspect daemon.log")
                        result = subprocess.run(
                            setup_report["next"]["doctor"], env=environment,
                            capture_output=True, text=True, encoding="utf-8", timeout=8,
                        )
                        if result.returncode == 0:
                            report = json.loads(result.stdout)
                            assert report["ok"] and report["data"]["fake"] is False, report
                            providers = report["data"]["providers"]["providers"]
                            for route in ("filesystem", "artifacts", "windows"):
                                match = [item for item in providers if item["route"] == route]
                                assert len(match) == 1, (route, providers)
                                assert match[0]["identity"]["id"] == f"windows-{route}", match[0]
                            return process
                        time.sleep(0.1)
                    raise TimeoutError("Broker did not become ready; inspect daemon.log")
                except BaseException:
                    if process.poll() is None:
                        process.terminate()
                    process.wait(timeout=5)
                    raise

            try:
                write_config(True)
                daemon = start_broker()
                assert (data_root / "state" / "audit").is_dir()
                assert (data_root / "runtime").is_dir()
                client = McpClient(snippet, mcp_log, environment)
                tools = client.request("tools/list", {})["tools"]
                assert any(tool["name"] == "semwright_execute" for tool in tools)
                args = {"root": "workspace", "path": "settings.json"}
                initial = check_ok(client.execute("filesystem.read", args))["text"]
                assert initial == before, (repr(initial), repr(before))
                first = '{"enabled": true}\n'
                check_ok(client.execute("filesystem.write", dict(args, text=first)))
                assert check_ok(client.execute("filesystem.read", args))["text"] == first
                assert data.read_text(encoding="utf-8") == first
                diff = "".join(difflib.unified_diff(
                    before.splitlines(True), first.splitlines(True),
                    fromfile="settings.json (before)", tofile="settings.json (after)",
                ))
                assert '-{"enabled": false}' in diff and '+{"enabled": true}' in diff
                (output / "workspace.diff").write_text(diff, encoding="utf-8")
                # Safe creation does not replace a same-name file the owner edited.
                new_text = {
                    "root": "workspace", "path": "created-only.txt",
                    "text": "new-only\n", "overwrite": False,
                }
                check_ok(client.execute("filesystem.write", new_text))
                new_file = workspace / "created-only.txt"
                assert new_file.read_bytes() == b"new-only\n"
                new_file.write_bytes(b"owner changed\n")
                occupied = client.execute("filesystem.write", new_text)
                assert not occupied["ok"], occupied
                assert occupied["error"]["code"] == "Conflict", occupied
                assert occupied["error"]["outcome_known"] is True, occupied
                assert new_file.read_bytes() == b"owner changed\n"
                bad_flag = client.execute("filesystem.write", dict(new_text, overwrite="false"))
                assert not bad_flag["ok"], bad_flag
                assert bad_flag["error"]["outcome_known"] is True, bad_flag
                assert new_file.read_bytes() == b"owner changed\n"
                assert not any(workspace.glob(".semwright-*.tmp"))
                for denied in (
                    {"root": "unknown", "path": "settings.json"},
                    {"root": "workspace", "path": "../outside/secret.txt"},
                    {"root": "workspace", "path": str(sentinel)},
                    {"root": "workspace", "path": "escape/secret.txt"},
                ):
                    check_denied(client.execute("filesystem.read", denied))
                    check_denied(client.execute("filesystem.write", dict(denied, text="forbidden")))
                assert sentinel.read_text(encoding="utf-8") == "outside must stay unchanged\n"
                transfer = {
                    "source_root": "handoff-source",
                    "source_path": "source.bin",
                    "destination_root": "handoff-destination",
                    "destination_path": "first.bin",
                    "overwrite": False,
                    "expected_sha256": hashlib.sha256(handoff_payload).hexdigest(),
                }
                published = client.execute("artifact.handoff", transfer)
                assert published["ok"], published
                assert published["execution"]["backend"] == "artifacts", published
                assert published["data"]["sha256"] == transfer["expected_sha256"]
                destination = artifact_destination / "first.bin"
                assert destination.read_bytes() == handoff_payload
                conflict = client.execute("artifact.handoff", transfer)
                assert not conflict["ok"], conflict
                assert conflict["error"]["code"] == "Conflict", conflict
                assert conflict["error"]["outcome_known"] is True, conflict
                assert destination.read_bytes() == handoff_payload
                tampered = dict(transfer, expected_sha256="0" * 64, destination_path="other.bin")
                rejected_digest = client.execute("artifact.handoff", tampered)
                assert not rejected_digest["ok"], rejected_digest
                assert rejected_digest["error"]["code"] == "Conflict", rejected_digest
                assert not (artifact_destination / "other.bin").exists()
                traversal = dict(transfer, destination_path="../outside/secret.txt")
                rejected_path = client.execute("artifact.handoff", traversal)
                assert not rejected_path["ok"], rejected_path
                assert rejected_path["error"]["outcome_known"] is True, rejected_path
                assert sentinel.read_text(encoding="utf-8") == "outside must stay unchanged\n"
                assert not any(artifact_destination.glob(".semwright-*.tmp"))
                client.close()
                client = McpClient(snippet, mcp_log, environment)
                second = '{"enabled": true, "second_task": true}\n'
                check_ok(client.execute("filesystem.write", dict(args, text=second)))
                assert check_ok(client.execute("filesystem.read", args))["text"] == second
                assert data.read_text(encoding="utf-8") == second
                daemon.terminate()
                daemon.wait(timeout=5)
                write_config(False)
                daemon = start_broker()
                # The MCP process still holds its old session. Refreshing that handshake
                # must not restore the old grant or redispatch either completed write.
                check_denied(client.execute("filesystem.write", dict(args, text="revoked")))
                check_denied(client.execute("filesystem.write", dict(
                    new_text, path="revoked-new-only.txt", text="revoked",
                )))
                assert not (workspace / "revoked-new-only.txt").exists()
                assert check_ok(client.execute("filesystem.read", args))["text"] == second
                assert data.read_text(encoding="utf-8") == second
                report = {
                    "evidence": "native-broker-mcp",
                    "hosted": os.environ.get("GITHUB_ACTIONS") == "true",
                    "setup_dry_run_and_non_overwrite": True,
                    "workspace_candidate_generated_without_activation": True,
                    "workspace_candidate_original_unchanged": True,
                    "workspace_candidate_duplicate_denied": True,
                    "generated_mcp_configuration_used": True,
                    "explicit_runtime_data_directory": True,
                    "fake": False,
                    "read_write_readback": True,
                    "new_only_text_created": True,
                    "new_only_text_human_edit_preserved": True,
                    "new_only_text_known_conflict": True,
                    "new_only_text_invalid_flag_denied": True,
                    "artifact_no_replace_native": True,
                    "artifact_no_replace_conflict": True,
                    "artifact_digest_guard": True,
                    "artifact_handoff_path_denied": True,
                    "diff": True,
                    "outside_and_junction_denied": True,
                    "second_task_after_reconnect": True,
                    "write_revoked_after_broker_restart": True,
                    "external_agent_host_accepted": False,
                    "interactive_desktop_accepted": False,
                }
                (output / "workspace.json").write_text(
                    json.dumps(report, indent=2) + "\n", encoding="utf-8",
                )
                print(json.dumps(report))
            finally:
                if client is not None:
                    client.close()
                if daemon is not None and daemon.poll() is None:
                    daemon.terminate()
                    daemon.wait(timeout=5)
                junction.rmdir()


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--bin-dir", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    options = parser.parse_args()
    run(options.bin_dir, options.output)
