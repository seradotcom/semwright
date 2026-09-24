#!/usr/bin/env python3
import hashlib, json, os, pathlib, shutil, socket, struct, subprocess, tempfile, time

ROOT = pathlib.Path(__file__).resolve().parents[3]
BIN_DIR = pathlib.Path(os.environ.get("BIN_DIR", ROOT / "target" / "debug"))
DRIVER = BIN_DIR / "semwright-godot-driver"
GODOT = pathlib.Path(os.environ["SEMWRIGHT_TEST_GODOT_BIN"]).resolve()
FIXTURE = ROOT / "integrations/godot/fixtures/basic"
PLUGIN = ROOT / "integrations/godot/addons/semwright"
CHILD_EVENTS = []
try:
    import tomllib
    VERSION = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
except Exception:
    VERSION = "0.9.0-dev.1"

if not DRIVER.is_file():
    raise SystemExit(f"missing Godot driver binary: {DRIVER}")
if not GODOT.is_file():
    raise SystemExit(f"missing pinned Godot binary: {GODOT}")

def file_sha(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()

def send(proc, value):
    data = json.dumps(value, separators=(",", ":"), ensure_ascii=False).encode()
    proc.stdin.write(struct.pack(">I", len(data)) + data)
    proc.stdin.flush()

def recv(proc):
    header = proc.stdout.read(4)
    if len(header) != 4:
        raise RuntimeError(f"short frame header {header!r}; stderr={proc.stderr.read().decode(errors='replace')}")
    n = struct.unpack(">I", header)[0]
    if n > 1_048_576:
        raise RuntimeError(f"oversized protocol frame {n}")
    data = proc.stdout.read(n)
    if len(data) != n:
        raise RuntimeError("short protocol frame body")
    return json.loads(data)

def request(proc, value, terminal_type=None):
    send(proc, value)
    progress = []
    while True:
        out = recv(proc)
        if out.get("type") == "event":
            CHILD_EVENTS.append(out)
            continue
        if out.get("type") == "progress" and out.get("id") == value.get("id"):
            progress.append(out)
            continue
        if terminal_type and out.get("type") != terminal_type:
            raise AssertionError((terminal_type, out))
        return out, progress

def digest(desc):
    return hashlib.sha256(json.dumps(desc, separators=(",", ":"), ensure_ascii=False).encode()).hexdigest()

def execute(proc, caps, name, args, rid):
    cap = caps[name]
    out, progress = request(proc, {
        "type": "execute",
        "id": rid,
        "command": name,
        "descriptor_sha256": digest(cap["descriptor"]),
        "args": args,
    })
    if out.get("type") == "failure":
        raise AssertionError(f"{name} failed: {out}")
    if out.get("type") != "result":
        raise AssertionError(out)
    return out["value"], progress

def free_port():
    s = socket.socket()
    s.bind(("127.0.0.1", 0))
    p = s.getsockname()[1]
    s.close()
    return p

def assert_no_godot_errors(label, result):
    stderr = str(result.get("stderr", ""))
    forbidden = ("ERROR:", "SCRIPT ERROR:", "Parse Error:")
    hits = [marker for marker in forbidden if marker in stderr]
    if hits:
        raise AssertionError(f"{label} emitted Godot errors {hits}: {stderr[:12000]}")

def stamp(value):
    return value["stamp"]

def V2(x, y):
    return {"$type": "Vector2", "value": [x, y]}

def V3(x, y, z):
    return {"$type": "Vector3", "value": [x, y, z]}

def Color(r, g, b, a=1):
    return {"$type": "Color", "value": [r, g, b, a]}

def Res(path):
    return {"$type": "Resource", "path": path}

with tempfile.TemporaryDirectory(prefix="semwright-godot-acceptance-") as td_raw:
    td = pathlib.Path(td_raw)
    project = td / "project"
    shutil.copytree(FIXTURE, project, ignore=shutil.ignore_patterns(".godot", "*.uid"))
    plugin_target = project / "addons" / "semwright"
    plugin_target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copytree(PLUGIN, plugin_target)
    for directory in ("scenes", "scripts", "assets"):
        (project / directory).mkdir(exist_ok=True)
    output = td / "artifacts"
    output.mkdir()
    home = td / "home"
    home.mkdir()
    project_id = hashlib.sha256(str(project.resolve()).encode()).hexdigest()
    secret = os.urandom(32).hex()
    port = free_port()
    movie_display = os.environ.get("SEMWRIGHT_TEST_GODOT_DISPLAY")
    config = td / "config.json"
    config.write_text(json.dumps({
        "port": port,
        "development_mode": True,
        "projects": [{"project": project_id, "root": str(project.resolve()), "secret": secret}],
        "runner": {
            "executable": str(GODOT.resolve()),
            "sha256": file_sha(GODOT),
            "output_root": str(output.resolve()),
            "display": movie_display,
        },
    }))
    os.chmod(config, 0o600)

    driver = subprocess.Popen(
        [str(DRIVER), "--config", str(config)],
        cwd=ROOT, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
    )
    godot = None
    godot_log = open(td / "godot-editor.log", "wb")
    trace = []
    try:
        ready, _ = request(driver, {
            "type": "hello", "protocol": 2,
            "provider": {
                "id": "driver:godot", "kind": "driver", "version": VERSION,
                "namespace": "driver.godot.", "application": None, "origin": "godot-real-acceptance",
            },
            "executable_sha256": "0" * 64,
        }, "ready")
        assert ready["protocol"] == 2
        interfaces, _ = request(driver, {"type": "interfaces", "id": "interfaces"}, "interfaces")
        assert interfaces["interfaces"]["cooperative_cancellation"] is True
        assert interfaces["interfaces"]["progress"] is True
        assert interfaces["interfaces"]["artifacts"] is True
        catalog, _ = request(driver, {"type": "capabilities", "id": "caps"}, "capabilities")
        caps = {x["descriptor"]["name"]: x for x in catalog["capabilities"]}
        assert len(caps) == 53, len(caps)

        env = os.environ.copy()
        env.update({
            "HOME": str(home),
            "XDG_DATA_HOME": str(home / "data"),
            "XDG_CONFIG_HOME": str(home / "config"),
            "XDG_CACHE_HOME": str(home / "cache"),
            "SEMWRIGHT_GODOT_PORT": str(port),
            "SEMWRIGHT_GODOT_PROJECT": project_id,
            "SEMWRIGHT_GODOT_SECRET": secret,
        })
        godot = subprocess.Popen(
            [str(GODOT), "--headless", "--editor", "--path", str(project)],
            env=env, stdout=godot_log, stderr=subprocess.STDOUT,
        )

        sessions = []
        for i in range(160):
            value, _ = execute(driver, caps, "driver.godot.session.list", {}, f"sessions-{i}")
            sessions = value["sessions"]
            if sessions:
                break
            if godot.poll() is not None:
                raise RuntimeError("Godot editor exited before pairing")
            time.sleep(0.05)
        assert len(sessions) == 1
        sid = sessions[0]["session"]

        def call(name, args):
            rid = f"op-{len(trace):03d}"
            value, progress = execute(driver, caps, name, args, rid)
            trace.append({"id": rid, "command": name, "args": args, "result": value, "progress": progress})
            return value

        status = call("driver.godot.project.inspect", {"session": sid})
        st = stamp(status)

        resources = [
            ("res://assets/floor_mesh.tres", "BoxMesh", [("size", V3(12, 0.2, 12))]),
            ("res://assets/floor_shape.tres", "BoxShape3D", [("size", V3(12, 0.2, 12))]),
            ("res://assets/player_shape.tres", "CapsuleShape3D", [("radius", 0.45), ("height", 1.8)]),
            ("res://assets/door_mesh.tres", "BoxMesh", [("size", V3(2.0, 3.0, 0.3))]),
            ("res://assets/door_shape.tres", "BoxShape3D", [("size", V3(2.0, 3.0, 0.3))]),
            ("res://assets/key_mesh.tres", "SphereMesh", [("radius", 0.35), ("height", 0.7)]),
            ("res://assets/key_shape.tres", "SphereShape3D", [("radius", 0.45)]),
            ("res://assets/floor_mat.tres", "StandardMaterial3D", [("albedo_color", Color(0.18, 0.22, 0.28))]),
            ("res://assets/door_mat.tres", "StandardMaterial3D", [("albedo_color", Color(0.55, 0.24, 0.08))]),
            ("res://assets/key_mat.tres", "StandardMaterial3D", [("albedo_color", Color(0.95, 0.72, 0.12))]),
        ]
        for path, klass, props in resources:
            value = call("driver.godot.resource.create", {
                "session": sid, "path": path, "class": klass,
                "properties": [{"name": k, "value": v} for k, v in props],
                "expect": st, "dry_run": False,
            })
            st = stamp(value)

        value = call("driver.godot.scene.create", {
            "session": sid, "path": "res://scenes/lab_room.tscn", "class": "Node3D",
            "name": "LabRoom", "expect": st, "dry_run": False,
        })
        st = stamp(value)

        def node(parent, klass, name, props=None):
            global st
            value = call("driver.godot.node.create", {
                "session": sid, "parent": parent, "class": klass, "name": name,
                "properties": [{"name": k, "value": v} for k, v in (props or [])],
                "expect": st, "dry_run": False,
            })
            st = stamp(value)

        node(".", "DirectionalLight3D", "Sun", [("rotation_degrees", V3(-55, -25, 0)), ("shadow_enabled", True)])
        node(".", "StaticBody3D", "Floor", [("position", V3(0, -0.1, 0))])
        node("Floor", "MeshInstance3D", "Mesh", [("mesh", Res("res://assets/floor_mesh.tres")), ("material_override", Res("res://assets/floor_mat.tres"))])
        node("Floor", "CollisionShape3D", "Collision", [("shape", Res("res://assets/floor_shape.tres"))])
        node(".", "CharacterBody3D", "Player", [("position", V3(0, 1, 4))])
        node("Player", "CollisionShape3D", "Collision", [("shape", Res("res://assets/player_shape.tres"))])
        node("Player", "Camera3D", "Camera", [("position", V3(0, 0.8, 0)), ("current", True)])
        node(".", "StaticBody3D", "Door", [("position", V3(0, 1.5, -4))])
        node("Door", "MeshInstance3D", "Mesh", [("mesh", Res("res://assets/door_mesh.tres")), ("material_override", Res("res://assets/door_mat.tres"))])
        node("Door", "CollisionShape3D", "Collision", [("shape", Res("res://assets/door_shape.tres"))])
        node(".", "Area3D", "Key", [("position", V3(2, 0.5, 0))])
        node("Key", "MeshInstance3D", "Mesh", [("mesh", Res("res://assets/key_mesh.tres")), ("material_override", Res("res://assets/key_mat.tres"))])
        node("Key", "CollisionShape3D", "Collision", [("shape", Res("res://assets/key_shape.tres"))])
        node(".", "AnimationPlayer", "Animations")
        node(".", "CanvasLayer", "HUD")
        node("HUD", "Label", "Status", [("text", "Find the key")])
        value = call("driver.godot.ui.layout", {
            "session": sid, "target": "HUD/Status",
            "anchor_left": 0.0, "anchor_top": 0.0, "anchor_right": 0.0, "anchor_bottom": 0.0,
            "offset_left": 24.0, "offset_top": 24.0, "offset_right": 300.0, "offset_bottom": 64.0,
            "grow_horizontal": 1, "grow_vertical": 1, "mouse_filter": 2,
            "minimum_size": V2(240, 40), "expect": st, "dry_run": False,
        })
        st = stamp(value)

        controller = """extends Node3D

var has_key := false

func _ready() -> void:
    print("SEMWRIGHT_LAB_READY")

func _on_key_body_entered(body: Node) -> void:
    if body.name != "Player":
        return
    has_key = true
    $HUD/Status.text = "Door unlocked"
    $Animations.play("door_open")
    $Key.queue_free()
"""
        player = """extends CharacterBody3D

const SPEED := 4.0

func _physics_process(_delta: float) -> void:
    var input := Input.get_vector("move_left", "move_right", "move_forward", "move_back")
    velocity.x = input.x * SPEED
    velocity.z = input.y * SPEED
    if not is_on_floor():
        velocity.y -= 0.4
    move_and_slide()
"""
        for path, source, target in [
            ("res://scripts/lab.gd", controller, "."),
            ("res://scripts/player.gd", player, "Player"),
        ]:
            value = call("driver.godot.script.write", {
                "session": sid, "path": path, "source": source, "expected_sha256": "",
                "expect": st, "dry_run": False,
            })
            st = stamp(value)
            value = call("driver.godot.script.attach", {
                "session": sid, "target": target, "path": path, "expect": st, "dry_run": False,
            })
            st = stamp(value)

        for name, code in [("move_forward", 87), ("move_back", 83), ("move_left", 65), ("move_right", 68)]:
            value = call("driver.godot.input.set", {
                "session": sid, "name": name, "deadzone": 0.5,
                "events": [{"type": "key", "code": code}], "expect": st, "dry_run": False,
            })
            st = stamp(value)

        value = call("driver.godot.signal.connect", {
            "session": sid, "source": "Key", "target": ".", "signal": "body_entered",
            "method": "_on_key_body_entered", "flags": 8, "expect": st, "dry_run": False,
        })
        st = stamp(value)

        value = call("driver.godot.animation.create", {
            "session": sid, "player": "Animations", "library": "", "animation": "door_open",
            "length": 1.0, "loop_mode": 0, "expect": st, "dry_run": False,
        })
        st = stamp(value)
        value = call("driver.godot.animation.track.add", {
            "session": sid, "player": "Animations", "library": "", "animation": "door_open",
            "type": "position_3d", "path": "Door", "enabled": True, "expect": st, "dry_run": False,
        })
        st = stamp(value)
        for t, pos in [(0.0, V3(0, 1.5, -4)), (1.0, V3(0, 4.5, -4))]:
            value = call("driver.godot.animation.keyframe.set", {
                "session": sid, "player": "Animations", "library": "", "animation": "door_open",
                "track": 0, "time": t, "value": pos, "transition": 1.0,
                "expect": st, "dry_run": False,
            })
            st = stamp(value)

        value = call("driver.godot.scene.save", {"session": sid, "expect": st, "dry_run": False})
        st = stamp(value)
        value = call("driver.godot.project.main_scene", {
            "session": sid, "path": "res://scenes/lab_room.tscn", "expect": st, "dry_run": False,
        })
        st = stamp(value)
        value = call("driver.godot.scene.reload", {"session": sid, "expect": st, "dry_run": False})
        st = stamp(value)
        time.sleep(0.25)

        scene = call("driver.godot.scene.inspect", {"session": sid})
        names = {x["name"] for x in scene["data"]["nodes"]}
        assert {"LabRoom", "Floor", "Player", "Door", "Key", "Animations", "HUD", "Status"}.issubset(names), sorted(names)

        validation, progress = execute(driver, caps, "driver.godot.project.validate", {"project": project_id}, "project-validate")
        trace.append({"id": "project-validate", "command": "driver.godot.project.validate", "args": {"project": project_id}, "result": validation, "progress": progress})
        assert validation["success"] and validation["exit_code"] == 0
        assert_no_godot_errors("project.validate", validation)
        assert len(progress) == 2 and progress[-1]["progress"]["completed"] == 1

        runtime, progress = execute(driver, caps, "driver.godot.project.run_test", {
            "project": project_id, "scene": "res://scenes/lab_room.tscn", "frames": 30,
        }, "project-run")
        trace.append({"id": "project-run", "command": "driver.godot.project.run_test", "args": {"project": project_id, "scene": "res://scenes/lab_room.tscn", "frames": 30}, "result": runtime, "progress": progress})
        assert runtime["success"] and "SEMWRIGHT_LAB_READY" in runtime["stdout"], runtime
        assert_no_godot_errors("project.run_test", runtime)
        assert len(progress) == 2

        packed, pack_progress = execute(driver, caps, "driver.godot.export.pack", {
            "project": project_id, "preset": "Semwright Pack", "output": "lab-room.pck",
        }, "export-pack")
        trace.append({"id": "export-pack", "command": "driver.godot.export.pack", "args": {"project": project_id, "preset": "Semwright Pack", "output": "lab-room.pck"}, "result": packed, "progress": pack_progress})
        assert packed["success"] and packed["artifact"] == "lab-room.pck", packed
        assert_no_godot_errors("export.pack", packed)
        assert (output / "lab-room.pck").is_file()
        artifact_frames = [frame for frame in pack_progress if frame.get("artifacts")]
        assert artifact_frames, pack_progress
        artifact = artifact_frames[-1]["artifacts"][0]
        assert artifact["reference"] == "artifact:godot:lab-room.pck"
        assert artifact["sha256"] == file_sha(output / "lab-room.pck")
        assert artifact["bytes"] == (output / "lab-room.pck").stat().st_size

        if movie_display:
            movie, movie_progress = execute(driver, caps, "driver.godot.movie.capture", {
                "project": project_id,
                "scene": "res://scenes/lab_room.tscn",
                "output": "lab-room.avi",
                "frames": 8,
                "fps": 8,
            }, "movie-capture")
            trace.append({
                "id": "movie-capture",
                "command": "driver.godot.movie.capture",
                "args": {
                    "project": project_id,
                    "scene": "res://scenes/lab_room.tscn",
                    "output": "lab-room.avi",
                    "frames": 8,
                    "fps": 8,
                },
                "result": movie,
                "progress": movie_progress,
            })
            assert movie["success"] and movie["artifact"] == "lab-room.avi", movie
            assert (output / "lab-room.avi").is_file()
            movie_artifacts = [frame for frame in movie_progress if frame.get("artifacts")]
            assert movie_artifacts, movie_progress
            movie_artifact = movie_artifacts[-1]["artifacts"][0]
            assert movie_artifact["reference"] == "artifact:godot:lab-room.avi"
            assert movie_artifact["sha256"] == file_sha(output / "lab-room.avi")

        cancel_id = "cancel-target"
        cap = caps["driver.godot.project.run_test"]
        send(driver, {
            "type": "execute",
            "id": cancel_id,
            "command": "driver.godot.project.run_test",
            "descriptor_sha256": digest(cap["descriptor"]),
            "args": {"project": project_id, "scene": "res://scenes/lab_room.tscn", "frames": 3600},
        })
        first_progress = recv(driver)
        assert first_progress["type"] == "progress" and first_progress["id"] == cancel_id
        send(driver, {"type": "cancel", "id": "cancel-request", "target": cancel_id})
        cancelled_ack = None
        cancelled_result = None
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline and (cancelled_ack is None or cancelled_result is None):
            frame = recv(driver)
            if frame.get("type") == "cancelled" and frame.get("id") == "cancel-request":
                cancelled_ack = frame
            elif frame.get("type") == "failure" and frame.get("id") == cancel_id:
                cancelled_result = frame
        assert cancelled_ack and cancelled_ack["accepted"] is True
        assert cancelled_result and cancelled_result["error"]["code"] == "Cancelled", cancelled_result
        assert CHILD_EVENTS, "real Godot produced no Driver Protocol child events"
        assert any(event.get("kind", "").startswith("godot.") for event in CHILD_EVENTS)

        trace_target = os.environ.get("SEMWRIGHT_GODOT_TRACE")
        if trace_target:
            trace_path = pathlib.Path(trace_target)
            trace_path.parent.mkdir(parents=True, exist_ok=True)
            trace_path.write_text(json.dumps({
                "executed": True,
                "godot_version": "4.7.2.stable.official.ed1daf0bf",
                "operations": trace,
                "events": CHILD_EVENTS,
            }, indent=2))
        print("REAL_GODOT_ACCEPTANCE_PASS", len(trace), "operations")
        print("runtime_marker=SEMWRIGHT_LAB_READY")
        print("nodes=", len(scene["data"]["nodes"]))
        print("runner_progress_frames=", len(progress))
        print("artifact_bytes=", (output / "lab-room.pck").stat().st_size)
        print("cooperative_cancellation=PASS")
        print("child_events=", len(CHILD_EVENTS))

        request(driver, {"type": "shutdown", "id": "shutdown"}, "shutdown")
        driver.wait(timeout=5)
    finally:
        if godot is not None and godot.poll() is None:
            godot.terminate()
            try:
                godot.wait(timeout=5)
            except subprocess.TimeoutExpired:
                godot.kill()
                godot.wait()
        godot_log.close()
        if driver.poll() is None:
            driver.kill()
            driver.wait()
