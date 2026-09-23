#!/usr/bin/env python3
import hashlib
import json
import os
import pathlib
import struct
import subprocess
import sys
import time
import tomllib

CRATE = pathlib.Path(__file__).resolve().parents[1]
ROOT = pathlib.Path(__file__).resolve().parents[3]
TARGET = pathlib.Path(os.environ.get("BIN_DIR", ROOT / "target" / "debug"))
DRIVER_VERSION = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))["workspace"]["package"]["version"]
DRIVER = TARGET / "semwright-figma-driver"
FAKE = TARGET / "semwright-fake-figma"

def send(proc, value):
    payload = json.dumps(value, separators=(",", ":"), ensure_ascii=False).encode()
    proc.stdin.write(struct.pack(">I", len(payload)) + payload)
    proc.stdin.flush()

def recv(proc):
    header = proc.stdout.read(4)
    if len(header) != 4:
        raise RuntimeError(f"short driver frame header: {header!r}")
    length = struct.unpack(">I", header)[0]
    if length > 1_048_576:
        raise RuntimeError("driver frame exceeded protocol limit")
    payload = proc.stdout.read(length)
    if len(payload) != length:
        raise RuntimeError("short driver frame body")
    return json.loads(payload)

def descriptor_digest(descriptor):
    encoded = json.dumps(descriptor, separators=(",", ":"), ensure_ascii=False).encode()
    return hashlib.sha256(encoded).hexdigest()

def request(proc, value, expected_type=None):
    send(proc, value)
    response = recv(proc)
    if expected_type and response.get("type") != expected_type:
        raise AssertionError((expected_type, response))
    return response

def execute(proc, caps, command, args, request_id):
    cap = caps[command]
    return request(proc, {
        "type": "execute",
        "id": request_id,
        "command": command,
        "descriptor_sha256": descriptor_digest(cap["descriptor"]),
        "args": args,
    })

def main():
    if not DRIVER.exists() or not FAKE.exists():
        raise SystemExit("build workspace first: cargo build --workspace")

    env = os.environ.copy()
    env["SEMWRIGHT_FIGMA_BRIDGE_PORT"] = "0"
    driver = subprocess.Popen(
        [str(DRIVER)],
        cwd=ROOT,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env=env,
    )
    fake = None
    try:
        ready = request(driver, {
            "type": "hello",
            "protocol": 1,
            "provider": {
                "id": "driver:figma",
                "kind": "driver",
                "version": DRIVER_VERSION,
                "namespace": "driver.figma.",
                "application": None,
                "origin": "figma-closeout-e2e",
            },
            "executable_sha256": "0" * 64,
        }, "ready")
        assert ready["id"] == "figma"

        catalog = request(driver, {"type": "capabilities", "id": "caps"}, "capabilities")
        caps = {cap["descriptor"]["name"]: cap for cap in catalog["capabilities"]}
        assert len(caps) == 91, len(caps)
        assert "driver.figma.node.search" in caps
        assert "driver.figma.layout.patch" in caps
        assert "driver.figma.design_system.extract" in caps
        assert "driver.figma.motion.keyframes.list" in caps
        assert "driver.figma.export.node" not in caps
        for cap in caps.values():
            name = cap["descriptor"]["name"]
            input_schema = cap["descriptor"]["input_schema"]
            output_schema = cap["descriptor"]["output_schema"]
            assert input_schema != {"not": {}}, name
            assert output_schema != {"not": {}}, name
            assert input_schema.get("type") == "object", name
            assert input_schema.get("additionalProperties") is False, name

        doctor = execute(driver, caps, "driver.figma.doctor", {}, "doctor")
        assert doctor["type"] == "result", doctor
        status = doctor["value"]
        assert "pairing_code" not in status
        assert status["pairing_required"] is True
        assert status["listen_host"] == "127.0.0.1"

        pairing = execute(driver, caps, "driver.figma.pairing.begin", {}, "pairing")
        assert pairing["type"] == "result", pairing
        secret = pairing["value"]["pairing_code"]
        port = pairing["value"]["listen_port"]
        assert len(secret) == 64
        assert pairing["value"]["listen_host"] == "127.0.0.1"

        fake_env = os.environ.copy()
        fake_env["SEMWRIGHT_FIGMA_PAIRING_SECRET"] = secret
        fake = subprocess.Popen(
            [str(FAKE), f"ws://127.0.0.1:{port}"],
            cwd=ROOT,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            env=fake_env,
        )

        sessions = []
        for _ in range(50):
            response = execute(driver, caps, "driver.figma.session.list", {}, "sessions")
            if response["type"] == "result" and response["value"]:
                sessions = response["value"]
                break
            time.sleep(0.05)
        assert len(sessions) == 1, sessions
        session_id = sessions[0]["session_id"]
        assert sessions[0]["revision"] == 0

        status = execute(
            driver,
            caps,
            "driver.figma.document.status",
            {"session_id": session_id, "expected_revision": 0},
            "status",
        )
        assert status["type"] == "result", status
        assert status["value"]["documentId"] == "fake-doc"

        created = execute(
            driver,
            caps,
            "driver.figma.frame.create",
            {
                "session_id": session_id,
                "expected_revision": 0,
                "name": "E2E Frame",
                "x": 40,
                "y": 60,
                "width": 320,
                "height": 180,
            },
            "create",
        )
        assert created["type"] == "result", created
        assert created["value"]["width"] == 320
        assert created["value"]["height"] == 180
        node_id = created["value"]["id"]

        layout = execute(
            driver,
            caps,
            "driver.figma.layout.patch",
            {
                "session_id": session_id,
                "expected_revision": 1,
                "nodeId": node_id,
                "layoutMode": "HORIZONTAL",
                "itemSpacing": 16,
            },
            "layout",
        )
        assert layout["type"] == "result", layout
        assert layout["value"]["layoutMode"] == "HORIZONTAL"

        snapshot = execute(
            driver,
            caps,
            "driver.figma.snapshot.subtree",
            {
                "session_id": session_id,
                "expected_revision": 2,
                "nodeId": node_id,
                "mode": "portable",
            },
            "snapshot",
        )
        assert snapshot["type"] == "result", snapshot
        assert "id" not in snapshot["value"], snapshot
        assert snapshot["value"]["name"] == "E2E Frame"

        stale = execute(
            driver,
            caps,
            "driver.figma.node.get",
            {"session_id": session_id, "expected_revision": 1, "nodeId": node_id},
            "stale",
        )
        assert stale["type"] == "failure", stale
        assert stale["error"]["code"] == "StaleReference", stale

        sessions = execute(driver, caps, "driver.figma.session.list", {}, "sessions2")
        revision = sessions["value"][0]["revision"]
        assert revision == 2, sessions

        fetched = execute(
            driver,
            caps,
            "driver.figma.node.get",
            {"session_id": session_id, "expected_revision": revision, "nodeId": node_id},
            "get",
        )
        assert fetched["type"] == "result", fetched
        assert fetched["value"]["name"] == "E2E Frame"

        collection = execute(
            driver, caps, "driver.figma.variable.collection.create",
            {"session_id": session_id, "expected_revision": 2, "name": "Theme"}, "collection",
        )
        assert collection["type"] == "result", collection
        collection_id = collection["value"]["id"]

        variable = execute(
            driver, caps, "driver.figma.variable.create",
            {
                "session_id": session_id, "expected_revision": 3,
                "collectionId": collection_id, "name": "brand/primary", "resolvedType": "COLOR",
            }, "variable",
        )
        assert variable["type"] == "result", variable
        variable_id = variable["value"]["id"]

        set_value = execute(
            driver, caps, "driver.figma.variable.set_value",
            {
                "session_id": session_id, "expected_revision": 4,
                "variableId": variable_id, "modeId": "m:1",
                "value": {"r": 1, "g": 0.2, "b": 0.1, "a": 1},
            }, "set-value",
        )
        assert set_value["type"] == "result", set_value

        design = execute(
            driver, caps, "driver.figma.design_system.extract",
            {"session_id": session_id, "expected_revision": 5}, "design",
        )
        assert design["type"] == "result", design
        assert design["value"]["collections"][0]["name"] == "Theme"
        assert design["value"]["variables"][0]["name"] == "brand/primary"

        reaction = execute(
            driver, caps, "driver.figma.prototype.reaction.set",
            {
                "session_id": session_id, "expected_revision": 5, "nodeId": node_id,
                "reactions": [{"trigger": {"type": "ON_CLICK"}, "actions": []}],
            }, "reaction",
        )
        assert reaction["type"] == "result", reaction
        listed = execute(
            driver, caps, "driver.figma.prototype.reaction.list",
            {"session_id": session_id, "expected_revision": 6, "nodeId": node_id}, "reaction-list",
        )
        assert len(listed["value"]) == 1

        motion_style = execute(
            driver, caps, "driver.figma.motion.style.apply",
            {
                "session_id": session_id, "expected_revision": 6, "nodeId": node_id,
                "styleId": "fake-spring", "duration": 0.4, "timelineOffset": 0,
            }, "motion-style",
        )
        assert motion_style["type"] == "result", motion_style

        keyframes = execute(
            driver, caps, "driver.figma.motion.keyframe.apply",
            {
                "session_id": session_id, "expected_revision": 7, "nodeId": node_id,
                "field": {"type": "x"},
                "track": {"keyframes": [{"t": 0, "value": 0}, {"t": 1, "value": 100}]},
            }, "motion-keyframe",
        )
        assert keyframes["type"] == "result", keyframes

        timeline = execute(
            driver, caps, "driver.figma.motion.timeline.set_duration",
            {
                "session_id": session_id, "expected_revision": 8, "nodeId": node_id,
                "timelineId": "main", "duration": 1.2,
            }, "motion-timeline",
        )
        assert timeline["type"] == "result", timeline
        motion = execute(
            driver, caps, "driver.figma.motion.node.inspect",
            {"session_id": session_id, "expected_revision": 9, "nodeId": node_id}, "motion-inspect",
        )
        assert len(motion["value"]["animationStyles"]) == 1
        assert len(motion["value"]["manualKeyframeTracks"]) == 1
        assert motion["value"]["timelines"][0]["duration"] == 1.2

        sticky = execute(
            driver, caps, "driver.figma.figjam.sticky.create",
            {"session_id": session_id, "expected_revision": 9, "name": "Agent"}, "sticky",
        )
        shape = execute(
            driver, caps, "driver.figma.figjam.shape.create",
            {"session_id": session_id, "expected_revision": 10, "name": "Semwright"}, "shape",
        )
        connector = execute(
            driver, caps, "driver.figma.figjam.connector.create",
            {
                "session_id": session_id, "expected_revision": 11,
                "from": sticky["value"]["id"], "to": shape["value"]["id"],
            }, "connector",
        )
        assert connector["type"] == "result", connector

        sessions = execute(driver, caps, "driver.figma.session.list", {}, "sessions-final")
        revision = sessions["value"][0]["revision"]
        assert revision == 12, sessions

        shutdown = request(driver, {"type": "shutdown", "id": "bye"}, "shutdown")
        assert shutdown["id"] == "bye"
        driver.wait(timeout=5)
        print(json.dumps({
            "status": "PASS",
            "capabilities": len(caps),
            "session_id": session_id,
            "final_revision": revision,
            "node_id": node_id,
        }, sort_keys=True))
        return 0
    finally:
        if fake is not None and fake.poll() is None:
            fake.terminate()
            try:
                fake.wait(timeout=3)
            except subprocess.TimeoutExpired:
                fake.kill()
        if driver.poll() is None:
            driver.terminate()
            try:
                driver.wait(timeout=3)
            except subprocess.TimeoutExpired:
                driver.kill()

if __name__ == "__main__":
    sys.exit(main())
