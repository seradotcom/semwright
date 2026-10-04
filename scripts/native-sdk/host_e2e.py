#!/usr/bin/env python3
"""Real owned CLI/MCP -> daemon -> Broker -> isolated Driver Host tests.

This script is CI-only. It never fabricates a Provider, copies exported artifacts
for admission, enables network, or changes the production sandbox policy.
"""
from __future__ import annotations

import hashlib
import json
import os
import selectors
import shutil
import signal
import subprocess
import tempfile
import time
import tomllib
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
BINS = ROOT / "target/debug"
EVIDENCE = ROOT / "verification/native-sdk/github-actions/native-host"


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def exact_request_digest(domain: str, value: object) -> str:
    def encode(item: object) -> str:
        if item is None or isinstance(item, (str, bool)):
            return json.dumps(item, ensure_ascii=False, separators=(",", ":"))
        if isinstance(item, int) and not isinstance(item, bool):
            if abs(item) > 9_007_199_254_740_991:
                raise AssertionError("request integer exceeds the shared exact range")
            return str(item)
        if isinstance(item, list):
            return "[" + ",".join(encode(child) for child in item) + "]"
        if isinstance(item, dict):
            if not all(isinstance(key, str) for key in item):
                raise AssertionError("request object keys must be strings")
            keys = sorted(item, key=lambda key: key.encode("utf-8"))
            return "{" + ",".join(
                json.dumps(key, ensure_ascii=False) + ":" + encode(item[key])
                for key in keys
            ) + "}"
        raise AssertionError("request digest only accepts exact JSON values")

    payload = domain + "\n" + encode(value)
    return hashlib.sha256(payload.encode("utf-8")).hexdigest()


def write_json(path: Path, value: object) -> None:
    with path.open("x", encoding="utf-8") as stream:
        json.dump(value, stream, ensure_ascii=False, indent=2)
    path.chmod(0o600)


class Fixture:
    def __init__(self, label: str, app: str = "scene", *, allow: bool = True,
                 data_write: bool = True, output_write: bool = True):
        self.label = label
        self.app = app
        self.provider = f"driver:native-{app}"
        self.command = f"driver.native-{app}."
        self.allow = allow
        self.data_write = data_write
        self.output_write = output_write
        self.temp = tempfile.TemporaryDirectory(prefix="semwright-native-owned-")
        self.root = Path(self.temp.name).resolve()
        self.root.chmod(0o700)
        self.paths = {}
        for name in ["runtime", "state", "home", "config", "binary", "native-data", "native-output", "native-runtime", "admitted", "protected"]:
            path = self.root / name
            path.mkdir(mode=0o700)
            self.paths[name] = path
        self.evidence = EVIDENCE / label
        self.evidence.mkdir(parents=True, exist_ok=False)
        self.records: list[dict] = []
        self.process: subprocess.Popen | None = None
        self.log = None
        self.attempt = 0
        self.socket = self.paths["runtime"] / "broker.sock"
        self.session = self.paths["runtime"] / "cli.session"
        self.env = {k: v for k, v in os.environ.items() if k not in {"DISPLAY", "WAYLAND_DISPLAY", "DBUS_SESSION_BUS_ADDRESS"}}
        self.env.update(HOME=str(self.paths["home"]), XDG_RUNTIME_DIR=str(self.paths["runtime"]), XDG_STATE_HOME=str(self.paths["state"]), RUST_BACKTRACE="1")
        executable = self.paths["binary"] / f"native-{app}"
        shutil.copyfile(BINS / "examples" / f"native-{app}", executable)
        executable.chmod(0o700)
        self.executable = executable
        self.version = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
        self.manifest = {
            "manifest_version": 1, "protocol": 4, "id": f"native-{app}",
            "version": self.version, "publisher": "semwright-owned-native-conformance",
            "executable": str(executable), "sha256": digest(executable),
            "application": {"desktop_id": None, "process_names": [f"native-{app}"], "supported_versions": []},
            "transport": "stdio_v1", "network": False,
            "mounts": [
                {"root": "native-data", "read_only": not data_write, "execute": False},
                {"root": "native-output", "read_only": not output_write, "execute": False},
            ],
            "resources": {"open_files": 128, "processes": 32, "cpu_seconds": 120,
                          "operation_cpu_seconds": 0, "address_space_bytes": 1_073_741_824,
                          "file_size_bytes": 16_777_216},
            "request_timeout_ms": 10_000,
            "interfaces": {"dynamic_capabilities": True, "cooperative_cancellation": True,
                           "events": True, "progress": False, "artifacts": False,
                           "health": True, "native_refs": True, "host_tools": False},
        }
        # Setup is owner/application provisioning only. Every tested mutation below
        # still crosses CLI/MCP -> daemon -> Broker/Policy -> real Driver Host.
        identities = {"driver": digest(executable), **{name: digest(BINS / name) for name in ["semwright", "semwrightd", "semwright-mcp", "semwright-sandbox"]}}
        if app == "inventory":
            bundle_source = ROOT / "verification/native-sdk/typescript/inventory.cjs"
            compiled_application = ROOT / "verification/native-sdk/typescript/compiled/examples/native-inventory/application.js"
            if not bundle_source.is_file() or not compiled_application.is_file():
                raise AssertionError("pinned TypeScript inventory build is missing")
            bundle = self.paths["native-runtime"] / "inventory.cjs"
            shutil.copyfile(bundle_source, bundle)
            bundle.chmod(0o400)
            source_node = Path(shutil.which("node") or "").resolve()
            if not source_node.is_file():
                raise AssertionError("canonical Node runtime is unavailable")
            # setup-node may install a group-writable cache binary. The Host must
            # not weaken its executable policy, so the owner stages exact bytes
            # into this fixture's private binary root and pins their digest.
            node = self.paths["binary"] / "node-runtime"
            shutil.copyfile(source_node, node)
            node.chmod(0o500)
            code = "import { Inventory } from " + json.dumps(compiled_application.as_uri()) + "; Inventory.provision(process.argv[1]);"
            result = subprocess.run(
                [str(node), "--input-type=module", "-e", code, str(self.paths["native-data"])],
                env=self.env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=10,
            )
            self.manifest["protocol"] = 5
            # Node/V8 reserves substantially more virtual address space than its
            # bounded JS heap. Match Semwright's already exercised Motion Canvas
            # Node ceiling without widening filesystem, network or process grants;
            # the bridge still caps V8 old-space at 256 MiB.
            self.manifest["resources"]["address_space_bytes"] = 4_294_967_296
            self.manifest["mounts"] = [
                {"root": "native-runtime", "read_only": True, "execute": False},
                {"root": "inventory-data", "read_only": not data_write, "execute": False},
            ]
            self.manifest["tools"] = [
                {
                    "root": "native-node",
                    "name": "node",
                    "sha256": digest(node),
                    "mounts": ["inventory-data"],
                }
            ]
            # Inventory has a static capability catalog. Its events command is
            # application polling, not the Driver protocol event stream.
            self.manifest["interfaces"]["dynamic_capabilities"] = False
            self.manifest["interfaces"]["events"] = False
            self.manifest["interfaces"]["host_tools"] = True
            grants = [
                ("native-runtime", self.paths["native-runtime"], False),
                ("inventory-data", self.paths["native-data"], data_write),
                ("native-node", node, False),
                ("admitted", self.paths["admitted"], True),
            ]
            identities.update({"node": digest(node), "bundle": digest(bundle)})
        else:
            result = subprocess.run([str(executable), "--init", str(self.paths["native-data"])], env=self.env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=10)
            grants = [
                ("native-data", self.paths["native-data"], data_write),
                ("native-output", self.paths["native-output"], output_write),
                ("admitted", self.paths["admitted"], True),
            ]
        if result.returncode:
            raise AssertionError(f"app initialization failed: {result.stderr.decode()}")
        write_json(self.paths["config"] / "driver.json", self.manifest)
        config = "drivers = [" + json.dumps(str(self.paths["config"] / "driver.json")) + "]\ndriver_network = false\n[policy]\nprofile = \"observe\"\n"
        config += "allow = " + json.dumps([self.provider, "project.manage"] if allow else []) + "\n"
        for name, path, writable in grants:
            config += "\n[[policy.filesystem]]\nname = " + json.dumps(name) + "\npath = " + json.dumps(str(path)) + "\nread = true\nwrite = " + str(writable).lower() + "\n"
        self.config = self.paths["config"] / "owner.toml"
        self.config.write_text(config)
        self.config.chmod(0o600)
        write_json(self.evidence / "binary-identities.json", identities)

    def __enter__(self):
        self.start()
        return self

    def __exit__(self, *_):
        self.close()

    def start(self):
        assert self.process is None
        self.attempt += 1
        self.log = (self.evidence / f"daemon-{self.attempt}.log").open("wb")
        self.process = subprocess.Popen([str(BINS / "semwrightd"), "--config", str(self.config), "--socket", str(self.socket)], env=self.env, stdin=subprocess.DEVNULL, stdout=self.log, stderr=self.log, start_new_session=True)
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            if self.process.poll() is not None:
                self.log.flush()
                raise AssertionError((self.evidence / f"daemon-{self.attempt}.log").read_text())
            if self.socket.is_socket():
                return
            time.sleep(0.02)
        raise AssertionError("real daemon did not expose its owner socket")

    def stop(self):
        if self.process is not None:
            pid = self.process.pid
            self.process.terminate()
            try:
                code = self.process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                os.killpg(pid, signal.SIGKILL)
                self.process.wait(timeout=5)
                raise AssertionError("daemon/Driver Host failed to shut down within deadline")
            finally:
                self.log.close()
                self.process = None
            if code != 0:
                raise AssertionError(f"daemon did not shut down cleanly: {code}")
            try:
                os.killpg(pid, 0)
            except ProcessLookupError:
                pass
            else:
                os.killpg(pid, signal.SIGKILL)
                raise AssertionError("owned driver descendants survived daemon shutdown")

    def close(self):
        try:
            self.stop()
        finally:
            (self.evidence / "calls.json").write_text(json.dumps(self.records, indent=2) + "\n")
            self.temp.cleanup()

    def invoke(self, command: str, args: dict | None = None, *, session: Path | None = None, ok: bool = True) -> dict:
        command = command if command == "doctor" or command.startswith(("driver.", "artifact.", "capabilities.", "job.", "jobs.", "project.")) else self.command + command
        result = subprocess.run([str(BINS / "semwright"), "--socket", str(self.socket), "--session-file", str(session or self.session), "--json", "execute", command, "--args-json", json.dumps(args or {}, separators=(",", ":"))], env=self.env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=15)
        if len(result.stdout) > 1_048_576:
            raise AssertionError("CLI output budget exceeded")
        try:
            value = json.loads(result.stdout)
        except Exception as error:
            raise AssertionError(f"CLI did not return an envelope: {result.stderr.decode()} {result.stdout[:2048]!r}") from error
        self.records.append({"transport": "cli", "command": command, "args": args or {}, "exit_code": result.returncode, "envelope": value})
        if value.get("ok") is not ok or ((result.returncode == 0) is not ok):
            raise AssertionError(json.dumps(value, indent=2))
        if ok and command.startswith(self.command):
            provenance = value["execution"]["provenance"]
            assert provenance["provider"] == self.provider and provenance["source"] == "driver"
            assert provenance["descriptor_sha256"] and provenance["provider_generation"] is not None
        return value

    def wait_job(self, job_id: str) -> dict:
        for _ in range(100):
            job = self.invoke("jobs.get", {"job_id": job_id})["data"]["job"]
            if job["state"] in {"succeeded", "failed", "cancelled"}:
                return job
            time.sleep(0.02)
        raise AssertionError("Broker job did not reach a terminal state")

    def inspect(self) -> dict:
        return self.invoke("inspect")["data"]

    def mutation(self, view: dict, key: str, parameters: dict) -> dict:
        return {"ref": view["ref"], "expected_revision": view["revision"], "expected_generation": view["generation"], "operation_key": key, "parameters": parameters}

    def inventory_observe(self, resource: str = "inventory") -> dict:
        return self.invoke("observe", {"resource": resource, "scope": "stock", "limit": 256})["data"]

    def inventory_mutation(self, view: dict, suffix: str, key: str, parameters: dict) -> dict:
        page = view["page"]
        if not page["items"]:
            raise AssertionError("inventory observation has no request epoch")
        expected = page["version"]
        epoch = page["items"][0]["request_epoch"]
        base = {
            "command": self.command + suffix,
            "app_version": "1.0.0",
            "expected": expected,
            "epoch": epoch,
            "key": key,
            "parameters": parameters,
        }
        request = {
            "resource": expected["resource"],
            "epoch": epoch,
            "key": key,
            "request_sha256": exact_request_digest("inventory-request/1", base),
        }
        return {"ref": view["ref"], "request": request, "parameters": parameters}

    def export(self, key: str, slot: str) -> dict:
        return self.invoke("export", self.mutation(self.inspect(), key, {"output_namespace": "owned", "slot": slot}))["data"]["artifact"]

    def admit(self, artifact: dict, destination: str, *, expected: str | None = None, ok: bool = True):
        # Only the canonical Broker command copies these bytes. The harness never
        # writes, copies or reconstructs an admitted application artifact.
        return self.invoke("artifact.handoff", {"source_root": "native-output", "source_path": artifact["path"], "destination_root": "admitted", "destination_path": destination, "max_bytes": 1_048_576, "expected_sha256": expected or artifact["sha256"], "media_type": artifact["mime_type"]}, ok=ok)


    def verify_admitted_json_text(self, artifact: dict, destination: str, *, check_id: str, pointer: str, expected: str) -> dict:
        # Spec preparation is pure canonical data preparation. The independent
        # verifier then reads only the Broker-admitted immutable artifact bytes.
        target = self.paths["admitted"] / destination
        definition = {
            "owner": {"session": "native-host-effects", "principal": "host_session"},
            "request_id": "host_effects_" + check_id,
            "source_digest": artifact["sha256"],
            "runtime_digest": digest(BINS / "semwright-native-effects"),
            "declared_producer_execution_status": "completed",
            "application_roots": [str(self.paths["native-data"])],
            "artifacts": [{
                "slot": "admitted", "path": destination,
                "sha256": artifact["sha256"], "bytes": target.stat().st_size,
                "mime_type": artifact["mime_type"],
            }],
            "checks": [{
                "id": check_id, "artifact_slot": "admitted",
                "selector": {"kind": "json", "pointer": pointer, "scalar": {"kind": "text"}},
                "predicate": {"kind": "equals", "expected": {"kind": "text", "value": expected}},
            }],
        }
        effects = BINS / "semwright-native-effects"
        prepared = subprocess.run(
            [str(effects), "--prepare"],
            input=json.dumps(definition, separators=(",", ":")).encode(),
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=10,
        )
        if prepared.returncode:
            raise AssertionError(prepared.stderr.decode())
        spec = self.paths["protected"] / (check_id + ".json")
        with spec.open("xb") as stream:
            stream.write(prepared.stdout.strip())
        spec.chmod(0o600)
        verified = subprocess.run(
            [str(effects), "--spec", str(spec), "--spec-sha256", digest(spec), "--artifact-root", str(self.paths["admitted"])],
            input=b"", stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=10,
        )
        if verified.returncode:
            raise AssertionError(verified.stderr.decode())
        result = json.loads(verified.stdout)
        if result.get("verdict") != "PASS" or result.get("execution_authority") is not False:
            raise AssertionError(result)
        write_json(self.evidence / ("effects-" + check_id + ".json"), result)
        return result


class McpClient:
    """A real framed JSON-RPC stdio client; not a direct Native SDK call."""
    def __init__(self, fixture: Fixture, label: str):
        self.fixture = fixture
        self.counter = 0
        self.session = fixture.paths["runtime"] / f"{label}.session"
        self.stderr = (fixture.evidence / f"{label}.stderr.log").open("wb")
        self.process = subprocess.Popen([str(BINS / "semwright-mcp"), "--socket", str(fixture.socket), "--session-file", str(self.session)], env=fixture.env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=self.stderr, bufsize=0)
        self.buffer = b""
        self.selector = selectors.DefaultSelector()
        self.selector.register(self.process.stdout, selectors.EVENT_READ)
        info = self.rpc("initialize", {"protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": {"name": "owned-native-sdk-client", "version": "1"}})
        assert info["serverInfo"]["name"] == "semwright"
        self.send({"jsonrpc": "2.0", "method": "notifications/initialized"})

    def send(self, message):
        self.process.stdin.write(json.dumps(message, separators=(",", ":")).encode() + b"\n")
        self.process.stdin.flush()

    def rpc(self, method: str, params: dict | None = None):
        self.counter += 1
        request_id = self.counter
        self.send({"jsonrpc": "2.0", "id": request_id, "method": method, "params": params or {}})
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            while b"\n" in self.buffer:
                line, self.buffer = self.buffer.split(b"\n", 1)
                if not line.strip():
                    continue
                message = json.loads(line)
                if message.get("id") == request_id:
                    if "error" in message:
                        raise AssertionError(message)
                    return message["result"]
            if not self.selector.select(timeout=max(0.0, deadline - time.monotonic())):
                break
            chunk = os.read(self.process.stdout.fileno(), 65_536)
            if not chunk:
                raise AssertionError("MCP server closed the framed stream")
            self.buffer += chunk
            if len(self.buffer) > 1_048_576:
                raise AssertionError("MCP frame budget exceeded")
        raise AssertionError("MCP response deadline exceeded")

    def execute(self, command: str, args: dict | None = None, *, ok: bool = True):
        command = command if command == "doctor" or command.startswith(("driver.", "artifact.", "capabilities.", "job.", "project.")) else self.fixture.command + command
        result = self.rpc("tools/call", {"name": "semwright_execute", "arguments": {"command": command, "args": args or {}}})
        value = result["structuredContent"]
        self.fixture.records.append({"transport": "mcp", "command": command, "envelope": value})
        assert value["ok"] is ok, value
        return value

    def close(self):
        self.selector.close()
        self.process.stdin.close()
        try:
            self.process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.process.terminate()
            self.process.wait(timeout=5)
        self.process.stdout.close()
        self.stderr.close()


class NativeHostTests(unittest.TestCase):
    def test_scene_cli_native_persistence_and_canonical_byte_admission(self):
        with Fixture("scene-cli") as fixture:
            doctor = fixture.invoke("doctor")["data"]
            self.assertFalse(doctor["fake"])
            before = fixture.inspect()
            self.assertIsInstance(before["ref"], str)
            changed = fixture.mutation(before, "scene_edit", {"object_id": "cube", "color": "#112233"})
            fixture.invoke("set-object", changed)
            stale = fixture.invoke("set-object", {**changed, "operation_key": "stale_edit"}, ok=False)
            self.assertEqual(stale["error"]["code"], "StaleReference")
            current = fixture.inspect()
            self.assertEqual(current["projection"]["objects"]["cube"]["color"], "#112233")
            self.assertEqual(current["projection"]["objects"]["independent"], before["projection"]["objects"]["independent"])
            artifact = fixture.export("scene_export", "scene")
            refused = fixture.admit(artifact, "rejected.json", expected="0" * 64, ok=False)
            self.assertEqual(refused["error"]["code"], "Conflict")
            self.assertFalse((fixture.paths["admitted"] / "rejected.json").exists())
            admitted = fixture.admit(artifact, "scene.json")["data"]
            target = fixture.paths["admitted"] / "scene.json"
            self.assertEqual(digest(target), artifact["sha256"])
            self.assertEqual(admitted["sha256"], artifact["sha256"])
            self.assertEqual(json.loads(target.read_text())["objects"]["cube"]["color"], "#112233")
            effect = fixture.verify_admitted_json_text(
                artifact, "scene.json", check_id="scene_color",
                pointer="/objects/cube/color", expected="#112233",
            )
            self.assertEqual(effect["declared_producer_execution_status"], "completed")

            # Continue through the canonical private Project Graph over the same
            # daemon/Broker session. Registration reconciles the Broker-admitted
            # immutable bytes; the Native SDK does not own or bypass Graph state.
            project = fixture.invoke("project.create", {"root": "admitted"})["data"]["project"]
            registered = fixture.invoke(
                "project.asset.register",
                {
                    "root": "admitted",
                    "project": project,
                    "label": "scene-export",
                    "resource_type": "native.scene.export",
                    "path": "scene.json",
                    "max_bytes": 1_048_576,
                },
            )["data"]["result"]
            asset = registered["asset"]["id"]
            provenance = fixture.invoke(
                "project.asset.provenance",
                {"root": "admitted", "project": project, "asset": asset, "limit": 16},
            )["data"]["result"]
            self.assertEqual(provenance["asset"]["asset"]["id"], asset)
            self.assertIsNone(provenance["producer"])
            self.assertEqual(provenance["asset"]["asset"]["locator"]["root"], "admitted")
            self.assertEqual(provenance["asset"]["asset"]["locator"]["relative_path"], "scene.json")
            self.assertNotIn("admitted", [mount["root"] for mount in fixture.manifest["mounts"]])

    def test_table_actual_host_recomputes_sum_and_exports_csv(self):
        with Fixture("table-cli", "table") as fixture:
            before = fixture.inspect()
            fixture.invoke("set-cell", fixture.mutation(before, "table_edit", {"cell": "A1", "value": 13}))
            after = fixture.inspect()
            self.assertNotEqual(after["revision"], before["revision"])
            current_rows = {row["cell"]: row["value"] for row in after["projection"]["rows"]}
            before_rows = {row["cell"]: row["value"] for row in before["projection"]["rows"]}
            self.assertEqual(current_rows["A3"], 33)
            self.assertEqual(current_rows["B1"], before_rows["B1"])
            artifact = fixture.export("table_export", "table")
            fixture.admit(artifact, "table.csv")
            self.assertIn("A3,33", (fixture.paths["admitted"] / "table.csv").read_text())
            cyclic = fixture.invoke("set-cell", fixture.mutation(fixture.inspect(), "cycle", {"cell": "A1", "value": {"sum": ["A3"]}}), ok=False)
            # A mutating failure reported across an untrusted Driver boundary is
            # intentionally uncertain. Prove current state with a fresh readback
            # instead of upgrading the transport failure into "no effect".
            self.assertFalse(cyclic["error"]["outcome_known"])
            self.assertEqual(fixture.inspect()["projection"], after["projection"])

    def test_policy_denial_does_not_mutate_or_hide_discovery(self):
        with Fixture("policy-denied", allow=False) as fixture:
            before = digest(fixture.paths["native-data"] / "document.json")
            denied = fixture.invoke("inspect", ok=False)
            self.assertEqual(denied["error"]["code"], "PolicyDenied")
            catalog = fixture.invoke("capabilities.search", {"query": "native-scene", "provider": fixture.provider, "limit": 32})
            self.assertTrue(catalog["data"]["capabilities"])
            self.assertEqual(digest(fixture.paths["native-data"] / "document.json"), before)
        # Table is a distinct domain/fixture and must be denied by the same Broker
        # boundary rather than inheriting Scene's result as evidence.
        with Fixture("table-policy-denied", "table", allow=False) as fixture:
            before = digest(fixture.paths["native-data"] / "document.json")
            denied = fixture.invoke("inspect", ok=False)
            self.assertEqual(denied["error"]["code"], "PolicyDenied")
            catalog = fixture.invoke(
                "capabilities.search",
                {"query": "native-table", "provider": fixture.provider, "limit": 32},
            )
            self.assertTrue(catalog["data"]["capabilities"])
            self.assertEqual(digest(fixture.paths["native-data"] / "document.json"), before)

    def test_limited_output_grant_cannot_be_upgraded_by_application(self):
        with Fixture("readonly-output", output_write=False) as fixture:
            before = fixture.inspect()
            result = fixture.invoke("export", fixture.mutation(before, "denied_export", {"output_namespace": "denied", "slot": "data"}), ok=False)
            self.assertFalse(result["ok"])
            self.assertEqual(list(fixture.paths["native-output"].iterdir()), [])
            self.assertEqual(fixture.inspect()["projection"], before["projection"])

    def test_core_job_wrapper_executes_native_read_through_same_broker_and_host(self):
        with Fixture("jobs") as fixture:
            started = fixture.invoke(
                "jobs.start",
                {"request": {"command": fixture.command + "inspect", "args": {}}},
            )["data"]["job"]
            self.assertEqual(started["state"], "queued")
            job = fixture.wait_job(started["id"])
            self.assertEqual(job["state"], "succeeded")
            self.assertEqual(job["command"], fixture.command + "inspect")
            self.assertTrue(job["result"]["ok"])
            self.assertEqual(
                job["result"]["execution"]["provenance"]["provider"],
                fixture.provider,
            )
            self.assertLessEqual(len(fixture.invoke("jobs.list")["data"]["jobs"]), 64)

    def test_mcp_protocol_uses_same_daemon_and_isolates_session_refs(self):
        with Fixture("mcp-real") as fixture:
            cli_view = fixture.inspect()
            mcp = McpClient(fixture, "mcp-a")
            other = None
            try:
                tools = mcp.rpc("tools/list")["tools"]
                self.assertTrue(any(tool["name"] == "semwright_execute" for tool in tools))
                self.assertFalse(any("approve" in tool["name"] for tool in tools))
                refused = mcp.execute("set-object", fixture.mutation(cli_view, "cross_cli", {"object_id": "cube", "color": "#000000"}), ok=False)
                self.assertIn(refused["error"]["code"], ["StaleReference", "NotFound"])
                view = mcp.execute("inspect")["data"]
                other = McpClient(fixture, "mcp-b")
                refused = other.execute("set-object", fixture.mutation(view, "cross_mcp", {"object_id": "cube", "color": "#000000"}), ok=False)
                self.assertIn(refused["error"]["code"], ["StaleReference", "NotFound"])
                mcp.execute("set-object", fixture.mutation(view, "mcp_edit", {"object_id": "cube", "color": "#abcdef"}))
                self.assertEqual(fixture.inspect()["projection"]["objects"]["cube"]["color"], "#abcdef")
            finally:
                if other:
                    other.close()
                mcp.close()

    def test_typescript_inventory_real_host_sqlite_recovery_events_and_private_publish(self):
        with Fixture("inventory-typescript", "inventory") as fixture:
            database = fixture.paths["native-data"] / "supply.sqlite3"
            initial_database_sha = digest(database)
            before = fixture.inventory_observe()
            version = before["page"]["version"]
            self.assertGreater(int(version["revision"]), 9_007_199_254_740_991)
            stock = {item["sku"]: item for item in before["page"]["items"]}
            reserve_args = fixture.inventory_mutation(before, "reserve", "host-reserve-alpha", {"sku": "alpha", "quantity": 7})
            reserved = fixture.invoke("reserve", reserve_args)["data"]
            self.assertFalse(reserved["deduplicated"])
            self.assertEqual(reserved["receipt"]["effect"]["available"], stock["alpha"]["available"] - 7)
            self.assertNotEqual(digest(database), initial_database_sha)

            current = fixture.inventory_observe()
            current_stock = {item["sku"]: item for item in current["page"]["items"]}
            self.assertEqual(current_stock["alpha"]["reserved"], stock["alpha"]["reserved"] + 7)
            self.assertNotEqual(current["page"]["version"]["revision"], version["revision"])

            recovered = fixture.invoke(
                "operation.get",
                {"ref": current["ref"], "request": reserve_args["request"]},
            )["data"]
            self.assertEqual(recovered["record"]["state"], "recorded")
            self.assertEqual(recovered["record"]["result"]["effect"]["sku"], "alpha")
            self.assertFalse(recovered["replay_allowed"])
            self.assertTrue(recovered["historical_only"])
            self.assertFalse(recovered["current_authority"])

            events = fixture.invoke(
                "events",
                {"ref": current["ref"], "resource": "inventory", "limit": 32},
            )["data"]
            self.assertFalse(events["resync_required"])
            self.assertTrue(events["historical_hints_only"])
            self.assertTrue(any(event["event"]["command"] == fixture.command + "reserve" for event in events["events"]))

            snapshot_args = fixture.inventory_mutation(current, "snapshot", "host-snapshot", {})
            snapshot = fixture.invoke("snapshot", snapshot_args)["data"]
            snapshot_id = snapshot["receipt"]["effect"]["snapshot_id"]
            self.assertEqual(len(snapshot_id), 64)

            after_snapshot = fixture.inventory_observe()
            publish_args = fixture.inventory_mutation(
                after_snapshot,
                "publish-private",
                "host-private-publish",
                {"snapshot_id": snapshot_id, "candidate_sha256": snapshot_id},
            )
            published = fixture.invoke("publish-private", publish_args)["data"]
            self.assertEqual(published["receipt"]["effect"]["publication"], "private-application-transaction")
            final = fixture.inventory_observe()
            self.assertNotEqual(final["page"]["version"]["revision"], after_snapshot["page"]["version"]["revision"])
            self.assertEqual(
                {item["sku"]: (item["available"], item["reserved"]) for item in final["page"]["items"]},
                {item["sku"]: (item["available"], item["reserved"]) for item in after_snapshot["page"]["items"]},
            )

    def test_restart_rebind_preserves_logical_identity_but_revokes_refs(self):
        with Fixture("restart") as fixture:
            initial = fixture.inspect()
            fixture.invoke("set-object", fixture.mutation(initial, "persist", {"object_id": "cube", "scale": 2.0}))
            before = fixture.inspect()
            fixture.stop()
            fixture.session.unlink(missing_ok=True)
            fixture.start()
            after = fixture.inspect()
            self.assertEqual(after["resource_id"], before["resource_id"])
            self.assertNotEqual(after["generation"], before["generation"])
            self.assertEqual(after["projection"], before["projection"])
            refused = fixture.invoke("set-object", fixture.mutation(before, "old_incarnation", {"object_id": "cube", "scale": 3.0}), ok=False)
            self.assertEqual(refused["error"]["code"], "StaleReference")


if __name__ == "__main__":
    if os.getenv("GITHUB_ACTIONS") != "true" or os.getenv("NATIVE_PRIVATE_REPOSITORY") != "true":
        raise SystemExit("This native Host acceptance runs only in private disposable Actions")
    if os.uname().sysname != "Linux" or not shutil.which("bwrap"):
        raise SystemExit("Linux bubblewrap is required; no skip or trusted fallback is accepted")
    for executable in ["semwright", "semwrightd", "semwright-mcp", "semwright-sandbox", "semwright-native-effects", "examples/native-scene", "examples/native-table", "examples/native-inventory"]:
        if not os.access(BINS / executable, os.X_OK):
            raise SystemExit(f"Missing exact-checkout executable {executable}")
    unittest.main(verbosity=2)
