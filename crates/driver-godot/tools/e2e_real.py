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


def execute_failure(proc, caps, name, args, rid):
    cap = caps[name]
    out, progress = request(proc, {
        "type": "execute",
        "id": rid,
        "command": name,
        "descriptor_sha256": digest(cap["descriptor"]),
        "args": args,
    })
    if out.get("type") != "failure":
        raise AssertionError(f"{name} unexpectedly succeeded: {out}")
    return out["error"], progress

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

def Quaternion(x, y, z, w):
    return {"$type": "Quaternion", "value": [x, y, z, w]}

def Transform3D(values):
    assert len(values) == 12
    return {"$type": "Transform3D", "value": list(values)}

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
    # A real imported source asset exercises EditorFileSystem/import semantics.
    (project / "assets" / "semantic_icon.svg").write_text(
        '<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16">'
        '<rect width="16" height="16" fill="#5b7cfa"/></svg>'
    )
    # A script-defined global class exercises ProjectSettings global class discovery
    # and Script metadata without invoking any discovered method.
    (project / "scripts" / "semantic_node.gd").write_text(
        "class_name SemwrightSemanticNode\n"
        "extends Node3D\n"
        "signal semantic_changed(value: int)\n"
        "@export var semantic_value: int = 7\n"
        "@export var semantic_transform: Transform3D = Transform3D.IDENTITY\n"
        "@export var semantic_vectors: PackedVector3Array = PackedVector3Array()\n"
        "@export var semantic_array: Array = []\n"
        "@export var semantic_dictionary: Dictionary = {}\n"
        "@export var semantic_node: Node\n"
        "const SEMANTIC_CONSTANT := 11\n"
        "func semantic_method(delta: float = 1.0) -> float:\n"
        "    return float(semantic_value) * delta\n"
    )
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
        assert len(caps) == 184, len(caps)

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
        matching = []
        # Hosted runners can be CPU/disk constrained while Godot performs its first
        # editor import. Wait for the authenticated session we actually paired rather
        # than assuming the plugin is ready within ~8 seconds.
        for i in range(600):
            value, _ = execute(driver, caps, "driver.godot.session.list", {}, f"sessions-{i}")
            sessions = value["sessions"]
            matching = [session for session in sessions if session.get("project") == project_id]
            if len(matching) == 1:
                break
            if len(matching) > 1:
                raise RuntimeError(f"multiple Godot sessions paired for disposable project: {matching!r}")
            if godot.poll() is not None:
                godot_log.flush()
                tail = (td / "godot-editor.log").read_text(errors="replace")[-8000:]
                raise RuntimeError(f"Godot editor exited before pairing (rc={godot.returncode}):\n{tail}")
            time.sleep(0.05)
        if len(matching) != 1:
            godot_log.flush()
            tail = (td / "godot-editor.log").read_text(errors="replace")[-8000:]
            raise RuntimeError(
                "timed out waiting for authenticated Godot plugin session; "
                f"sessions={sessions!r}\nGodot log tail:\n{tail}"
            )
        sid = matching[0]["session"]

        def call(name, args):
            rid = f"op-{len(trace):03d}"
            value, progress = execute(driver, caps, name, args, rid)
            trace.append({"id": rid, "command": name, "args": args, "result": value, "progress": progress})
            return value

        status = call("driver.godot.project.inspect", {"session": sid})
        st = stamp(status)

        def mutate(name, args):
            global st
            payload = dict(args)
            payload.update({"session": sid, "expect": st, "dry_run": False})
            value = call(name, payload)
            st = stamp(value)
            return value

        # Versioned introspection is descriptive only. It can search and describe
        # metadata but cannot invoke discovered methods.
        api_matches = call("driver.godot.api.search", {
            "session": sid, "query": "Camera3D", "base": "Node", "limit": 32,
        })
        assert any(row["name"] == "Camera3D" for row in api_matches["data"]["classes"])
        api_node3d = call("driver.godot.api.describe", {
            "session": sid, "class": "Node3D", "include_inherited": True,
            "properties": True, "methods": True, "signals": True, "enums": True,
        })
        assert api_node3d["data"]["engine_version"].startswith("4.7.2")
        assert any(row["name"] == "transform" for row in api_node3d["data"]["properties"])
        assert any(row["name"] == "translate" for row in api_node3d["data"]["methods"])

        project_classes = None
        for _ in range(100):
            project_classes = call("driver.godot.project.class.list", {
                "session": sid, "query": "SemwrightSemanticNode", "limit": 32,
            })
            if any(row["name"] == "SemwrightSemanticNode" for row in project_classes["data"]["classes"]):
                break
            time.sleep(0.05)
        assert project_classes is not None
        assert any(row["name"] == "SemwrightSemanticNode" for row in project_classes["data"]["classes"])
        project_class = call("driver.godot.project.class.describe", {
            "session": sid, "name": "SemwrightSemanticNode",
        })
        assert project_class["data"]["base"] == "Node3D"
        assert project_class["data"]["metadata_available"] is True
        assert project_class["data"]["tool"] is False
        assert any(row["name"] == "semantic_value" for row in project_class["data"]["properties"])
        assert any(row["name"] == "semantic_method" for row in project_class["data"]["methods"])
        assert any(row["name"] == "semantic_changed" for row in project_class["data"]["signals"])

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
            ("res://assets/semantic_mat.tres", "StandardMaterial3D", []),
            ("res://assets/environment.tres", "Environment", []),
            ("res://assets/particles.tres", "ParticleProcessMaterial", []),
            ("res://assets/tileset.tres", "TileSet", []),
            ("res://assets/navigation.tres", "NavigationMesh", []),
            ("res://assets/navigation2d.tres", "NavigationPolygon", []),
            ("res://assets/player_shape2d.tres", "RectangleShape2D", [("size", V2(24, 24))]),
            ("res://assets/area_shape2d.tres", "CircleShape2D", [("radius", 12.0)]),
            ("res://assets/ui_theme.tres", "Theme", []),
            ("res://assets/state_machine.tres", "AnimationNodeStateMachine", []),
            ("res://assets/mesh_library.tres", "MeshLibrary", []),
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
        node(".", "AnimationTree", "AnimationTree", [("tree_root", Res("res://assets/state_machine.tres"))])
        node(".", "MultiplayerSpawner", "Spawner")
        node(".", "MultiplayerSynchronizer", "Synchronizer")
        node(".", "MultiplayerSynchronizer", "SynchronizerDry")
        node(".", "TileMapLayer", "Tiles", [("tile_set", Res("res://assets/tileset.tres"))])
        node(".", "GridMap", "Grid", [("mesh_library", Res("res://assets/mesh_library.tres"))])
        node(".", "Path3D", "Rail3D")
        node("Rail3D", "PathFollow3D", "Follower")
        node(".", "NavigationRegion3D", "NavRegion", [("navigation_mesh", Res("res://assets/navigation.tres"))])
        node("Player", "NavigationAgent3D", "Agent")
        node(".", "NavigationLink3D", "NavLink")
        node(".", "Node2D", "TwoD")
        node("TwoD", "CharacterBody2D", "Player2D", [("position", V2(64, 64))])
        node("TwoD/Player2D", "CollisionShape2D", "Collision", [("shape", Res("res://assets/player_shape2d.tres"))])
        node("TwoD/Player2D", "NavigationAgent2D", "Agent")
        node("TwoD", "NavigationRegion2D", "NavRegion", [("navigation_polygon", Res("res://assets/navigation2d.tres"))])
        node("TwoD", "NavigationLink2D", "NavLink")
        node("TwoD", "Area2D", "Area")
        node("TwoD/Area", "CollisionShape2D", "Collision", [("shape", Res("res://assets/area_shape2d.tres"))])
        node("TwoD", "StaticBody2D", "AnchorA")
        node("TwoD", "StaticBody2D", "AnchorB")
        node("TwoD", "PinJoint2D", "Joint")
        node("TwoD", "Path2D", "Rail2D")
        node("TwoD/Rail2D", "PathFollow2D", "Follower")
        node(".", "AudioStreamPlayer", "Music")
        node(".", "GPUParticles3D", "Particles", [("process_material", Res("res://assets/particles.tres"))])
        node(".", "Skeleton3D", "Rig")
        node("Rig", "BoneAttachment3D", "Attachment")
        node(".", "CanvasLayer", "HUD")
        node("HUD", "Label", "Status", [("text", "Find the key")])
        node(".", "Node3D", "SemanticNode")
        mutate("driver.godot.script.attach", {
            "target": "SemanticNode", "path": "res://scripts/semantic_node.gd",
        })
        value = call("driver.godot.ui.layout", {
            "session": sid, "target": "HUD/Status",
            "anchor_left": 0.0, "anchor_top": 0.0, "anchor_right": 0.0, "anchor_bottom": 0.0,
            "offset_left": 24.0, "offset_top": 24.0, "offset_right": 300.0, "offset_bottom": 64.0,
            "grow_horizontal": 1, "grow_vertical": 1, "mouse_filter": 2,
            "minimum_size": V2(240, 40), "expect": st, "dry_run": False,
        })
        st = stamp(value)

        # Generic semantic substrate: structured Variant values round-trip through
        # type-aware property writes. Wrong Variant types fail without changing state.
        transform_values = [1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 4]
        mutate("driver.godot.node.patch", {
            "target": "Player",
            "properties": [{"name": "transform", "value": Transform3D(transform_values)}],
        })
        player_state = call("driver.godot.node.inspect", {"session": sid, "path": "Player"})
        encoded_transform = player_state["data"]["properties"]["transform"]
        assert encoded_transform["$type"] == "Transform3D"
        assert len(encoded_transform["value"]) == 12

        semantic_transform = [1, 0, 0, 0, 1, 0, 0, 0, 1, 2, 3, 4]
        semantic_array = {
            "$type": "Array",
            "value": [
                1,
                {"$type": "Vector3", "value": [7.0, 8.0, 9.0]},
                {"$type": "StringName", "value": "semantic"},
            ],
        }
        semantic_dictionary = {
            "$type": "Dictionary",
            "entries": [
                {"key": "mode", "value": 2},
                {
                    "key": {"$type": "StringName", "value": "target"},
                    "value": {"$type": "Vector2", "value": [2.5, 4.5]},
                },
            ],
        }
        mutate("driver.godot.node.patch", {
            "target": "SemanticNode",
            "properties": [
                {"name": "semantic_value", "value": 23},
                {"name": "semantic_transform", "value": Transform3D(semantic_transform)},
                {
                    "name": "semantic_vectors",
                    "value": {
                        "$type": "PackedVector3Array",
                        "value": [[1.0, 2.0, 3.0], [-4.0, 5.0, 6.0]],
                    },
                },
                {"name": "semantic_array", "value": semantic_array},
                {"name": "semantic_dictionary", "value": semantic_dictionary},
                {"name": "semantic_node", "value": {"$type": "NodeRef", "path": "Player/Camera"}},
            ],
        })
        semantic_state = call("driver.godot.node.inspect", {
            "session": sid, "path": "SemanticNode",
        })
        semantic_props = semantic_state["data"]["properties"]
        assert semantic_props["semantic_value"] == 23
        assert semantic_props["semantic_transform"] == {
            "$type": "Transform3D", "value": semantic_transform,
        }
        assert semantic_props["semantic_vectors"] == {
            "$type": "PackedVector3Array",
            "value": [[1.0, 2.0, 3.0], [-4.0, 5.0, 6.0]],
        }
        assert semantic_props["semantic_array"] == semantic_array
        assert semantic_props["semantic_dictionary"] == semantic_dictionary
        assert semantic_props["semantic_node"]["$type"] == "NodeRef"
        assert semantic_props["semantic_node"]["path"] == "Player/Camera"
        assert semantic_props["semantic_node"]["class"] == "Camera3D"

        # Recursive decoding is authoritative even where JSON Schema intentionally
        # stays bounded rather than recursively self-referential.
        before_nested_bad_write = st.copy()
        nested_bad, _ = execute_failure(driver, caps, "driver.godot.node.patch", {
            "session": sid,
            "target": "SemanticNode",
            "properties": [{
                "name": "semantic_array",
                "value": {
                    "$type": "Array",
                    "value": [{"$type": "ArbitraryObject", "value": []}],
                },
            }],
            "expect": st,
            "dry_run": False,
        }, "nested-variant-reject")
        assert nested_bad["code"] == "InvalidArgument", nested_bad
        after_nested_bad_write = call("driver.godot.project.inspect", {"session": sid})
        assert stamp(after_nested_bad_write) == before_nested_bad_write

        before_bad_write = st.copy()
        bad_write, _ = execute_failure(driver, caps, "driver.godot.node.patch", {
            "session": sid,
            "target": "Player/Camera",
            "properties": [{"name": "fov", "value": V3(1, 2, 3)}],
            "expect": st,
            "dry_run": False,
        }, "typed-write-reject")
        assert bad_write["code"] == "InvalidArgument", bad_write
        after_bad_write = call("driver.godot.project.inspect", {"session": sid})
        assert stamp(after_bad_write) == before_bad_write

        # Semantic-domain acceptance: the generic Node/Resource substrate is not enough.
        # Exercise typed domain operations through the production bridge before save/reload.

        mutate("driver.godot.tileset.configure", {
            "path": "res://assets/tileset.tres", "tile_size": [32, 32],
            "tile_shape": 0, "tile_layout": 0, "tile_offset_axis": 0, "uv_clipping": False,
        })
        tileset = call("driver.godot.tileset.inspect", {"session": sid, "path": "res://assets/tileset.tres"})
        assert tileset["data"]["tile_size"] == [32, 32]
        mutate("driver.godot.tilemap.clear", {"target": "Tiles"})
        tilemap = call("driver.godot.tilemap.inspect", {"session": sid, "target": "Tiles"})
        assert tilemap["data"]["tile_set"] == "res://assets/tileset.tres"

        # 3D grid authoring is method-backed in Godot and must not rely on node.patch.
        mutate("driver.godot.meshlibrary.item.create", {
            "path": "res://assets/mesh_library.tres", "id": 0, "name": "FloorBlock",
            "mesh": "res://assets/floor_mesh.tres",
            "navigation_mesh": "res://assets/navigation.tres", "navigation_layers": 1,
        })
        mutate("driver.godot.meshlibrary.item.configure", {
            "path": "res://assets/mesh_library.tres", "id": 0,
            "name": "FloorBlockSemantic", "navigation_layers": 3,
        })
        mutate("driver.godot.meshlibrary.item.create", {
            "path": "res://assets/mesh_library.tres", "id": 1, "name": "Temporary",
            "mesh": "res://assets/door_mesh.tres",
        })
        mutate("driver.godot.meshlibrary.item.remove", {
            "path": "res://assets/mesh_library.tres", "id": 1,
        })
        library = call("driver.godot.meshlibrary.inspect", {
            "session": sid, "path": "res://assets/mesh_library.tres",
        })
        assert library["data"]["items"][0]["name"] == "FloorBlockSemantic"
        mutate("driver.godot.gridmap.configure", {
            "target": "Grid", "mesh_library": "res://assets/mesh_library.tres",
            "cell_size": [2.0, 2.0, 2.0], "cell_octant_size": 8, "cell_scale": 1.0,
            "cell_center_x": True, "cell_center_y": True, "cell_center_z": True,
            "bake_navigation": False, "collision_layer": 1, "collision_mask": 1,
        })
        mutate("driver.godot.gridmap.cell.set", {
            "target": "Grid", "position": [0, 0, 0], "item": 0, "orientation": 0,
        })
        mutate("driver.godot.gridmap.cell.set", {
            "target": "Grid", "position": [1, 0, 0], "item": 0, "orientation": 1,
        })
        mutate("driver.godot.gridmap.cell.erase", {
            "target": "Grid", "position": [1, 0, 0],
        })
        grid = call("driver.godot.gridmap.inspect", {"session": sid, "target": "Grid"})
        assert grid["data"]["cells"] == [{"position": [0, 0, 0], "item": 0, "orientation": 0}]
        mutate("driver.godot.gridmap.clear", {"target": "Grid"})
        mutate("driver.godot.gridmap.cell.set", {
            "target": "Grid", "position": [0, 0, 0], "item": 0, "orientation": 0,
        })

        # Bézier path authoring covers both 3D and 2D plus typed follower state.
        mutate("driver.godot.path.configure", {
            "target": "Rail3D", "bake_interval": 0.2, "closed": False, "up_vector_enabled": True,
        })
        mutate("driver.godot.path.point.add", {
            "target": "Rail3D", "position": [0.0, 0.0, 0.0],
            "out": [1.0, 0.0, 0.0], "tilt": 0.0,
        })
        mutate("driver.godot.path.point.add", {
            "target": "Rail3D", "position": [4.0, 1.0, 0.0],
            "in": [-1.0, 0.0, 0.0], "out": [1.0, 0.0, 1.0], "tilt": 0.1,
        })
        mutate("driver.godot.path.point.add", {
            "target": "Rail3D", "position": [8.0, 1.0, 2.0],
            "in": [-1.0, 0.0, -1.0], "tilt": 0.2,
        })
        mutate("driver.godot.path.point.configure", {
            "target": "Rail3D", "index": 1, "position": [4.0, 1.5, 0.0],
            "in": [-1.0, 0.0, 0.0], "out": [1.0, 0.0, 1.0], "tilt": 0.15,
        })
        mutate("driver.godot.path.point.remove", {"target": "Rail3D", "index": 2})
        path3d = call("driver.godot.path.inspect", {"session": sid, "target": "Rail3D"})
        assert path3d["data"]["dimension"] == 3 and len(path3d["data"]["points"]) == 2
        assert path3d["data"]["baked_length"] > 0.0
        mutate("driver.godot.path.follow.configure", {
            "target": "Rail3D/Follower", "progress_ratio": 0.5, "loop": True,
            "cubic_interp": True, "rotation_mode": 3, "tilt_enabled": True,
            "use_model_front": False, "h_offset": 0.0, "v_offset": 0.0,
        })
        follow3d = call("driver.godot.path.follow.inspect", {
            "session": sid, "target": "Rail3D/Follower",
        })
        assert follow3d["data"]["dimension"] == 3

        mutate("driver.godot.path.configure", {
            "target": "TwoD/Rail2D", "bake_interval": 4.0,
        })
        mutate("driver.godot.path.point.add", {
            "target": "TwoD/Rail2D", "position": [0.0, 0.0], "out": [32.0, 0.0],
        })
        mutate("driver.godot.path.point.add", {
            "target": "TwoD/Rail2D", "position": [96.0, 48.0], "in": [-32.0, 0.0],
        })
        path2d = call("driver.godot.path.inspect", {"session": sid, "target": "TwoD/Rail2D"})
        assert path2d["data"]["dimension"] == 2 and path2d["data"]["baked_length"] > 0.0
        mutate("driver.godot.path.follow.configure", {
            "target": "TwoD/Rail2D/Follower", "progress_ratio": 0.25,
            "loop": False, "cubic_interp": True, "rotates": True,
            "h_offset": 0.0, "v_offset": 0.0,
        })
        follow2d = call("driver.godot.path.follow.inspect", {
            "session": sid, "target": "TwoD/Rail2D/Follower",
        })
        assert follow2d["data"]["dimension"] == 2 and follow2d["data"]["rotates"] is True
        mutate("driver.godot.path.clear", {"target": "TwoD/Rail2D"})
        mutate("driver.godot.path.point.add", {
            "target": "TwoD/Rail2D", "position": [0.0, 0.0],
        })
        mutate("driver.godot.path.point.add", {
            "target": "TwoD/Rail2D", "position": [96.0, 48.0],
        })

        mutate("driver.godot.navigation.region.configure", {
            "target": "NavRegion", "enabled": True, "navigation_layers": 1,
            "enter_cost": 0.0, "travel_cost": 1.0, "use_edge_connections": True,
            "navigation_mesh": "res://assets/navigation.tres",
        })
        region = call("driver.godot.navigation.region.inspect", {"session": sid, "target": "NavRegion"})
        assert region["data"]["navigation_mesh"] == "res://assets/navigation.tres"
        mutate("driver.godot.navigation.agent.configure", {
            "target": "Player/Agent", "navigation_layers": 1, "target_position": [0.0, 1.0, -4.0],
            "path_desired_distance": 0.5, "target_desired_distance": 0.5,
            "path_max_distance": 5.0, "radius": 0.5, "height": 1.8, "max_speed": 4.0,
            "avoidance_enabled": True, "avoidance_layers": 1, "avoidance_mask": 1,
            "avoidance_priority": 0.5, "neighbor_distance": 10.0, "max_neighbors": 8,
            "use_3d_avoidance": True,
        })
        agent = call("driver.godot.navigation.agent.inspect", {"session": sid, "target": "Player/Agent"})
        assert agent["data"]["avoidance_enabled"] is True
        mutate("driver.godot.navigation.link.configure", {
            "target": "NavLink", "enabled": True, "bidirectional": True, "navigation_layers": 1,
            "enter_cost": 0.0, "travel_cost": 1.0,
            "start_position": [-1.0, 0.0, 0.0], "end_position": [1.0, 0.0, 0.0],
        })

        mutate("driver.godot.physics.body.configure", {
            "target": "Player", "collision_layer": 1, "collision_mask": 1,
            "motion_mode": 0, "max_slides": 4, "floor_stop_on_slope": True,
            "floor_max_angle": 0.785398, "floor_snap_length": 0.1,
            "wall_min_slide_angle": 0.261799, "up_direction": [0.0, 1.0, 0.0],
            "velocity": [0.0, 0.0, 0.0],
        })
        body = call("driver.godot.physics.body.inspect", {"session": sid, "target": "Player"})
        assert body["data"]["class"] == "CharacterBody3D"
        mutate("driver.godot.physics.area.configure", {
            "target": "Key", "monitoring": True, "monitorable": True, "priority": 0.0,
            "gravity_point": False, "gravity": 9.8, "gravity_space_override": 0,
            "linear_damp_space_override": 0, "linear_damp": 0.1,
            "angular_damp_space_override": 0, "angular_damp": 0.1,
            "audio_bus_override": False, "audio_bus_name": "Master",
            "collision_layer": 1, "collision_mask": 1,
        })
        area = call("driver.godot.physics.area.inspect", {"session": sid, "target": "Key"})
        assert area["data"]["monitoring"] is True
        mutate("driver.godot.collision.shape.configure", {
            "target": "Player/Collision", "shape": "res://assets/player_shape.tres", "disabled": False,
        })

        # The same semantic navigation/physics capabilities must preserve 2D meaning
        # instead of falling back to generic node.property mutation.
        mutate("driver.godot.navigation.region.configure", {
            "target": "TwoD/NavRegion", "enabled": True, "navigation_layers": 2,
            "enter_cost": 0.25, "travel_cost": 1.25, "use_edge_connections": True,
            "navigation_polygon": "res://assets/navigation2d.tres",
        })
        region2d = call("driver.godot.navigation.region.inspect", {
            "session": sid, "target": "TwoD/NavRegion",
        })
        assert region2d["data"]["dimension"] == "2d"
        assert region2d["data"]["navigation_polygon"] == "res://assets/navigation2d.tres"
        bake2d = call("driver.godot.navigation.region.bake", {
            "session": sid, "target": "TwoD/NavRegion", "expect": st, "dry_run": True,
        })
        assert bake2d["applied"] is False

        mutate("driver.godot.navigation.agent.configure", {
            "target": "TwoD/Player2D/Agent", "navigation_layers": 2,
            "target_position": [128.0, 96.0], "path_desired_distance": 2.0,
            "target_desired_distance": 3.0, "path_max_distance": 64.0,
            "radius": 8.0, "max_speed": 120.0, "avoidance_enabled": True,
            "avoidance_layers": 2, "avoidance_mask": 2, "avoidance_priority": 0.4,
            "neighbor_distance": 48.0, "max_neighbors": 6,
            "time_horizon_agents": 1.5, "time_horizon_obstacles": 1.0,
        })
        agent2d = call("driver.godot.navigation.agent.inspect", {
            "session": sid, "target": "TwoD/Player2D/Agent",
        })
        assert agent2d["data"]["dimension"] == "2d"
        assert agent2d["data"]["target_position"] == [128.0, 96.0]

        mutate("driver.godot.navigation.link.configure", {
            "target": "TwoD/NavLink", "enabled": True, "bidirectional": True,
            "navigation_layers": 2, "enter_cost": 0.0, "travel_cost": 1.0,
            "start_position": [8.0, 16.0], "end_position": [96.0, 16.0],
        })

        mutate("driver.godot.physics.body.configure", {
            "target": "TwoD/Player2D", "collision_layer": 2, "collision_mask": 2,
            "motion_mode": 0, "max_slides": 4, "floor_stop_on_slope": True,
            "floor_max_angle": 0.785398, "floor_snap_length": 1.0,
            "wall_min_slide_angle": 0.261799, "up_direction": [0.0, -1.0],
            "velocity": [24.0, 0.0],
        })
        body2d = call("driver.godot.physics.body.inspect", {
            "session": sid, "target": "TwoD/Player2D",
        })
        assert body2d["data"]["dimension"] == "2d"
        assert body2d["data"]["velocity"] == [24.0, 0.0]

        mutate("driver.godot.physics.area.configure", {
            "target": "TwoD/Area", "monitoring": True, "monitorable": True,
            "priority": 0.0, "gravity_point": False, "gravity": 980.0,
            "gravity_direction": [0.0, 1.0],
            "gravity_point_unit_distance": 0.0, "gravity_space_override": 0,
            "linear_damp_space_override": 0, "linear_damp": 0.1,
            "angular_damp_space_override": 0, "angular_damp": 0.1,
            "audio_bus_override": False, "audio_bus_name": "Master",
            "collision_layer": 2, "collision_mask": 2,
        })
        area2d = call("driver.godot.physics.area.inspect", {
            "session": sid, "target": "TwoD/Area",
        })
        assert area2d["data"]["dimension"] == "2d"
        assert area2d["data"]["gravity_direction"] == [0.0, 1.0], area2d["data"]

        mutate("driver.godot.physics.area.configure", {
            "target": "TwoD/Area", "gravity_point": True,
            "gravity_point_center": [32.0, 48.0], "gravity_point_unit_distance": 64.0,
        })
        point_area2d = call("driver.godot.physics.area.inspect", {
            "session": sid, "target": "TwoD/Area",
        })
        assert point_area2d["data"]["gravity_point"] is True
        assert point_area2d["data"]["gravity_point_center"] == [32.0, 48.0]

        mutate("driver.godot.physics.area.configure", {
            "target": "TwoD/Area", "gravity_point": False,
            "gravity_direction": [0.0, 1.0],
        })

        mutate("driver.godot.collision.shape.configure", {
            "target": "TwoD/Player2D/Collision",
            "shape": "res://assets/player_shape2d.tres", "disabled": False,
        })
        mutate("driver.godot.physics.joint.configure", {
            "target": "TwoD/Joint", "node_a": "../AnchorA", "node_b": "../AnchorB",
            "bias": 0.2, "exclude_nodes_from_collision": True,
        })

        mutate("driver.godot.audio.bus.create", {
            "name": "SemwrightSFX", "position": -1, "volume_db": -3.0, "send": "Master",
        })
        mutate("driver.godot.audio.bus.configure", {
            "name": "SemwrightSFX", "volume_db": -2.0, "mute": False, "solo": False, "send": "Master",
        })
        buses = call("driver.godot.audio.bus.inspect", {"session": sid})
        assert any(bus["name"] == "SemwrightSFX" for bus in buses["data"]["buses"])
        mutate("driver.godot.audio.player.configure", {
            "target": "Music", "bus": "SemwrightSFX", "volume_db": -6.0,
            "pitch_scale": 1.0, "autoplay": False, "max_polyphony": 2,
        })
        player_audio = call("driver.godot.audio.player.inspect", {"session": sid, "target": "Music"})
        assert player_audio["data"]["bus"] == "SemwrightSFX"

        mutate("driver.godot.particles.material.configure", {
            "path": "res://assets/particles.tres", "direction": [0.0, 1.0, 0.0],
            "gravity": [0.0, -1.0, 0.0], "color": [0.9, 0.7, 0.2, 1.0],
            "emission_box_extents": [0.25, 0.25, 0.25], "emission_shape": 3,
            "spread": 30.0, "initial_velocity_min": 1.0, "initial_velocity_max": 2.0,
            "scale_min": 0.5, "scale_max": 1.0, "lifetime_randomness": 0.1,
        })
        mutate("driver.godot.particles.configure", {
            "target": "Particles", "amount": 16, "lifetime": 1.5, "emitting": False,
            "one_shot": False, "preprocess": 0.0, "randomness": 0.1, "speed_scale": 1.0,
            "amount_ratio": 1.0, "explosiveness": 0.0, "local_coords": False,
            "fixed_fps": 30, "use_fixed_seed": True, "seed": 42,
            "process_material": "res://assets/particles.tres",
        })
        particles = call("driver.godot.particles.inspect", {"session": sid, "target": "Particles"})
        assert particles["data"]["amount"] == 16
        mutate("driver.godot.particles.restart", {"target": "Particles"})

        mutate("driver.godot.environment.configure", {
            "path": "res://assets/environment.tres", "background_mode": 1,
            "background_color": [0.03, 0.04, 0.06, 1.0], "background_energy_multiplier": 1.0,
            "ambient_light_source": 3, "ambient_light_color": [0.4, 0.45, 0.5, 1.0],
            "ambient_light_energy": 0.8, "fog_enabled": False, "fog_density": 0.01,
            "fog_light_color": [0.5, 0.55, 0.6, 1.0], "fog_light_energy": 1.0,
            "glow_enabled": False, "glow_intensity": 0.8, "tonemap_mode": 2,
        })
        environment = call("driver.godot.environment.inspect", {"session": sid, "path": "res://assets/environment.tres"})
        assert environment["data"]["tonemap_mode"] == 2
        mutate("driver.godot.material.standard.configure", {
            "path": "res://assets/semantic_mat.tres", "albedo_color": [0.15, 0.55, 0.8, 1.0],
            "metallic": 0.2, "roughness": 0.45, "emission_enabled": False,
            "emission": [0.0, 0.0, 0.0, 1.0], "transparency": 0, "shading_mode": 1,
        })
        material = call("driver.godot.material.standard.inspect", {"session": sid, "path": "res://assets/semantic_mat.tres"})
        assert abs(material["data"]["roughness"] - 0.45) < 0.001
        mutate("driver.godot.camera.configure", {
            "target": "Player/Camera", "projection": 0, "fov": 70.0,
            "near": 0.1, "far": 200.0, "keep_aspect": 1, "current": True,
            "cull_mask": 0xFFFFF, "environment": "res://assets/environment.tres",
        })
        camera = call("driver.godot.camera.inspect", {"session": sid, "target": "Player/Camera"})
        assert abs(camera["data"]["fov"] - 70.0) < 0.001
        mutate("driver.godot.light.configure", {
            "target": "Sun", "color": [1.0, 0.95, 0.85, 1.0], "energy": 1.2,
            "indirect_energy": 1.0, "specular": 0.5, "volumetric_fog_energy": 1.0,
            "shadow_enabled": True, "cull_mask": 0xFFFFF,
        })
        light = call("driver.godot.light.inspect", {"session": sid, "target": "Sun"})
        assert light["data"]["shadow_enabled"] is True

        mutate("driver.godot.ui.control.configure", {
            "target": "HUD/Status", "visible": True, "focus_mode": 0, "mouse_filter": 2,
            "layout_direction": 0, "size_flags_horizontal": 1, "size_flags_vertical": 1,
            "size_flags_stretch_ratio": 1.0, "tooltip_text": "Semwright semantic UI",
            "theme_type_variation": "", "minimum_size": [240.0, 40.0],
        })
        control = call("driver.godot.ui.control.inspect", {"session": sid, "target": "HUD/Status"})
        assert control["data"]["visible"] is True
        mutate("driver.godot.ui.text.configure", {
            "target": "HUD/Status", "text": "Find the semantic key",
            "horizontal_alignment": 0, "vertical_alignment": 1,
            "autowrap_mode": 0, "text_overrun_behavior": 0, "uppercase": False,
        })
        mutate("driver.godot.theme.configure", {
            "path": "res://assets/ui_theme.tres", "kind": "color",
            "theme_type": "Label", "name": "font_color", "color": [0.9, 0.95, 1.0, 1.0],
        })
        mutate("driver.godot.theme.apply", {
            "target": "HUD/Status", "theme": "res://assets/ui_theme.tres", "type_variation": "",
        })
        theme = call("driver.godot.theme.inspect", {"session": sid, "path": "res://assets/ui_theme.tres"})
        assert "Label" in theme["data"]["types"]

        mutate("driver.godot.skeleton.bone.add", {
            "target": "Rig", "name": "root", "parent": -1,
            "position": [0.0, 0.0, 0.0], "rotation": [0.0, 0.0, 0.0, 1.0],
            "scale": [1.0, 1.0, 1.0],
        })
        mutate("driver.godot.skeleton.bone.configure", {
            "target": "Rig", "index": 0, "name": "root", "parent": -1, "enabled": True,
            "position": [0.0, 0.1, 0.0], "rotation": [0.0, 0.0, 0.0, 1.0],
            "scale": [1.0, 1.0, 1.0], "reset_pose": False,
        })
        mutate("driver.godot.skeleton.attachment.configure", {
            "target": "Rig/Attachment", "bone_name": "root", "bone_idx": 0,
            "override_pose": False, "use_external_skeleton": False, "external_skeleton": "",
        })
        skeleton = call("driver.godot.skeleton.inspect", {"session": sid, "target": "Rig"})
        assert skeleton["data"]["bone_count"] == 1

        # Project-level semantic authoring.
        mutate("driver.godot.project.window.configure", {
            "viewport_width": 960, "viewport_height": 540,
            "resizable": True, "borderless": False, "always_on_top": False,
        })
        window = call("driver.godot.project.window.inspect", {"session": sid})
        assert window["data"]["viewport_width"] == 960
        mutate("driver.godot.project.rendering.configure", {"taa": False, "use_debanding": False})
        call("driver.godot.project.rendering.inspect", {"session": sid})
        mutate("driver.godot.project.physics.configure", {
            "ticks_per_second": 60, "max_steps_per_frame": 8, "jitter_fix": 0.5,
            "gravity_2d": 980.0, "gravity_3d": 9.8, "gravity_vector_3d": [0.0, -1.0, 0.0],
        })
        physics_project = call("driver.godot.project.physics.inspect", {"session": sid})
        assert physics_project["data"]["ticks_per_second"] == 60
        mutate("driver.godot.project.layers.set", {"kind": "3d_physics", "index": 1, "name": "World"})
        layers = call("driver.godot.project.layers.inspect", {"session": sid})
        assert any(row["name"] == "World" for row in layers["data"]["3d_physics"])

        # Asset/import semantics use a real SVG imported by the editor.
        asset = call("driver.godot.asset.inspect", {"session": sid, "path": "res://assets/semantic_icon.svg"})
        assert asset["data"]["import_sidecar"] is True
        call("driver.godot.asset.dependencies", {"session": sid, "path": "res://assets/semantic_icon.svg"})
        imported = call("driver.godot.asset.import.inspect", {"session": sid, "path": "res://assets/semantic_icon.svg"})
        scalar_param = next(
            ((k, v) for k, v in imported["data"]["params"].items()
             if isinstance(v, (bool, int, float, str))),
            None,
        )
        if scalar_param is not None:
            mutate("driver.godot.asset.import.configure", {
                "path": "res://assets/semantic_icon.svg",
                "params": [{"key": scalar_param[0], "value": scalar_param[1]}],
            })
        mutate("driver.godot.asset.reimport", {"path": "res://assets/semantic_icon.svg"})

        # Non-secret export preset semantics.
        presets = call("driver.godot.export.preset.list", {"session": sid})
        assert any(preset["name"] == "Semwright Pack" for preset in presets["data"]["presets"])
        mutate("driver.godot.export.preset.configure", {
            "name": "Semwright Pack", "custom_features": "semwright_ci",
        })
        preset = call("driver.godot.export.preset.inspect", {"session": sid, "name": "Semwright Pack"})
        assert preset["data"]["custom_features"] == "semwright_ci"

        # Localization resources and project registration.
        mutate("driver.godot.translation.create", {
            "path": "res://assets/es_translation.translation", "locale": "es", "register": True,
        })
        mutate("driver.godot.translation.message.set", {
            "path": "res://assets/es_translation.translation", "source": "HELLO",
            "translation": "Hola", "context": "",
        })
        mutate("driver.godot.translation.message.set", {
            "path": "res://assets/es_translation.translation", "source": "OPEN",
            "translation": "Abrir", "context": "menu",
        })
        mutate("driver.godot.translation.message.set", {
            "path": "res://assets/es_translation.translation", "source": "TEMP",
            "translation": "Temporal", "context": "",
        })
        mutate("driver.godot.translation.message.remove", {
            "path": "res://assets/es_translation.translation", "source": "TEMP", "context": "",
        })
        translation = call("driver.godot.translation.inspect", {
            "session": sid, "path": "res://assets/es_translation.translation",
        })
        assert any(row["source"] == "HELLO" and row["translation"] == "Hola"
                   for row in translation["data"]["messages"])
        assert any(row["source"] == "OPEN" and row["context"] == "menu"
                   and row["translation"] == "Abrir"
                   for row in translation["data"]["messages"])
        mutate("driver.godot.localization.configure", {
            "fallback": "en", "test_locale": "",
            "translations": ["res://assets/es_translation.translation"],
        })
        localization = call("driver.godot.localization.inspect", {"session": sid})
        assert "res://assets/es_translation.translation" in localization["data"]["translations"]

        # Deep AnimationTree state-machine authoring.
        mutate("driver.godot.animation_tree.state.add", {
            "tree": "AnimationTree", "name": "Idle", "kind": "animation",
            "position": [0.0, 0.0], "animation": "door_open",
        })
        mutate("driver.godot.animation_tree.state.add", {
            "tree": "AnimationTree", "name": "Run", "kind": "animation",
            "position": [220.0, 0.0], "animation": "door_open",
        })
        mutate("driver.godot.animation_tree.state.add", {
            "tree": "AnimationTree", "name": "Temporary", "kind": "animation",
            "position": [440.0, 0.0], "animation": "door_open",
        })
        mutate("driver.godot.animation_tree.transition.add", {
            "tree": "AnimationTree", "from": "Idle", "to": "Run",
            "advance_condition": "go", "advance_expression": "", "advance_mode": 2,
            "priority": 1, "reset": True, "switch_mode": 0, "xfade_time": 0.1,
        })
        mutate("driver.godot.animation_tree.transition.configure", {
            "tree": "AnimationTree", "from": "Idle", "to": "Run",
            "advance_condition": "go", "advance_expression": "", "advance_mode": 2,
            "priority": 2, "reset": True, "switch_mode": 0, "xfade_time": 0.2,
        })
        mutate("driver.godot.animation_tree.transition.add", {
            "tree": "AnimationTree", "from": "Run", "to": "Temporary",
            "advance_condition": "", "advance_expression": "", "advance_mode": 1,
            "priority": 1, "reset": True, "switch_mode": 0, "xfade_time": 0.0,
        })
        mutate("driver.godot.animation_tree.transition.remove", {
            "tree": "AnimationTree", "from": "Run", "to": "Temporary",
        })
        tree = call("driver.godot.animation_tree.inspect", {"session": sid, "tree": "AnimationTree"})
        assert any(row["name"] == "Idle" for row in tree["data"]["nodes"])
        assert any(row["from"] == "Idle" and row["to"] == "Run" for row in tree["data"]["transitions"])
        mutate("driver.godot.animation_tree.parameter.set", {
            "tree": "AnimationTree", "parameter": "conditions/go", "value": False,
        })
        tree = call("driver.godot.animation_tree.inspect", {"session": sid, "tree": "AnimationTree"})
        assert any(row["name"] == "conditions/go" and row["value"] is False
                   for row in tree["data"]["parameters"])
        mutate("driver.godot.animation_tree.state.remove", {
            "tree": "AnimationTree", "name": "Temporary",
        })

        # Recursive AnimationTree graph authoring: StateMachine -> BlendTree ->
        # blend nodes and named 1D/2D blend spaces.
        mutate("driver.godot.animation_tree.state.add", {
            "tree": "AnimationTree", "name": "BlendGraph", "kind": "blend_tree",
            "position": [660.0, 0.0],
        })
        blend_root = call("driver.godot.animation_tree.node.inspect", {
            "session": sid, "tree": "AnimationTree", "graph": "BlendGraph",
        })
        assert blend_root["data"]["class"] == "AnimationNodeBlendTree"

        for name, position in [("IdleClip", [0.0, 0.0]), ("RunClip", [0.0, 120.0])]:
            mutate("driver.godot.animation_tree.blend_tree.node.add", {
                "tree": "AnimationTree", "graph": "BlendGraph", "name": name,
                "kind": "animation", "position": position, "animation": "door_open",
            })
        mutate("driver.godot.animation_tree.blend_tree.node.add", {
            "tree": "AnimationTree", "graph": "BlendGraph", "name": "Blend",
            "kind": "blend2", "position": [220.0, 60.0], "sync": True,
        })
        for input_node, input_index, output_node in [
            ("Blend", 0, "IdleClip"),
            ("Blend", 1, "RunClip"),
            ("output", 0, "Blend"),
        ]:
            mutate("driver.godot.animation_tree.blend_tree.connection.set", {
                "tree": "AnimationTree", "graph": "BlendGraph",
                "input_node": input_node, "input_index": input_index,
                "output_node": output_node, "connected": True,
            })
        blend_tree = call("driver.godot.animation_tree.blend_tree.inspect", {
            "session": sid, "tree": "AnimationTree", "graph": "BlendGraph",
        })
        assert {"IdleClip", "RunClip", "Blend"}.issubset(
            {row["name"] for row in blend_tree["data"]["nodes"]}
        )
        mutate("driver.godot.animation_tree.blend_tree.node.configure", {
            "tree": "AnimationTree", "graph": "BlendGraph", "name": "Blend",
            "position": [240.0, 70.0], "sync": False,
        })
        blend_node = call("driver.godot.animation_tree.node.inspect", {
            "session": sid, "tree": "AnimationTree", "graph": "BlendGraph/Blend",
        })
        assert blend_node["data"]["class"] == "AnimationNodeBlend2"
        assert blend_node["data"]["sync"] is False

        mutate("driver.godot.animation_tree.blend_tree.node.add", {
            "tree": "AnimationTree", "graph": "BlendGraph", "name": "SpeedSpace",
            "kind": "blend_space_1d", "position": [460.0, 0.0],
            "min_space": -1.0, "max_space": 1.0, "snap": 0.1,
        })
        mutate("driver.godot.animation_tree.blend_space.configure", {
            "tree": "AnimationTree", "graph": "BlendGraph/SpeedSpace",
            "min_space": -2.0, "max_space": 2.0, "snap": 0.25,
            "value_label": "Speed", "blend_mode": 0, "sync_mode": 0,
            "cyclic_length": 0.0,
        })
        for name, position in [("Slow", -1.0), ("Fast", 1.0), ("Temporary", 0.0)]:
            mutate("driver.godot.animation_tree.blend_space.point.add", {
                "tree": "AnimationTree", "graph": "BlendGraph/SpeedSpace",
                "name": name, "kind": "animation", "position": position,
                "animation": "door_open", "index": -1,
            })
        mutate("driver.godot.animation_tree.blend_space.point.configure", {
            "tree": "AnimationTree", "graph": "BlendGraph/SpeedSpace",
            "index": 1, "name": "Sprint", "position": 1.5,
            "animation": "door_open",
        })
        speed_space = call("driver.godot.animation_tree.blend_space.inspect", {
            "session": sid, "tree": "AnimationTree", "graph": "BlendGraph/SpeedSpace",
        })
        assert speed_space["data"]["dimension"] == 1
        assert [row["name"] for row in speed_space["data"]["points"]] == [
            "Slow", "Sprint", "Temporary"
        ]
        mutate("driver.godot.animation_tree.blend_space.point.remove", {
            "tree": "AnimationTree", "graph": "BlendGraph/SpeedSpace", "index": 2,
        })

        mutate("driver.godot.animation_tree.blend_tree.node.add", {
            "tree": "AnimationTree", "graph": "BlendGraph", "name": "DirectionSpace",
            "kind": "blend_space_2d", "position": [460.0, 220.0],
            "min_space": [-1.0, -1.0], "max_space": [1.0, 1.0],
            "snap": [0.1, 0.1], "auto_triangles": False,
        })
        for name, position in [
            ("Left", [-1.0, 0.0]),
            ("Right", [1.0, 0.0]),
            ("Forward", [0.0, 1.0]),
        ]:
            mutate("driver.godot.animation_tree.blend_space.point.add", {
                "tree": "AnimationTree", "graph": "BlendGraph/DirectionSpace",
                "name": name, "kind": "animation", "position": position,
                "animation": "door_open", "index": -1,
            })
        mutate("driver.godot.animation_tree.blend_space.triangle.add", {
            "tree": "AnimationTree", "graph": "BlendGraph/DirectionSpace",
            "a": 0, "b": 1, "c": 2, "index": -1,
        })
        direction_space = call("driver.godot.animation_tree.blend_space.inspect", {
            "session": sid, "tree": "AnimationTree",
            "graph": "BlendGraph/DirectionSpace",
        })
        assert direction_space["data"]["dimension"] == 2
        assert len(direction_space["data"]["triangles"]) == 1
        mutate("driver.godot.animation_tree.blend_space.triangle.remove", {
            "tree": "AnimationTree", "graph": "BlendGraph/DirectionSpace", "index": 0,
        })

        # Exercise disconnect plus reconnect with a different root source.
        mutate("driver.godot.animation_tree.blend_tree.connection.set", {
            "tree": "AnimationTree", "graph": "BlendGraph",
            "input_node": "output", "input_index": 0,
            "output_node": "Blend", "connected": False,
        })
        mutate("driver.godot.animation_tree.blend_tree.connection.set", {
            "tree": "AnimationTree", "graph": "BlendGraph",
            "input_node": "output", "input_index": 0,
            "output_node": "SpeedSpace", "connected": True,
        })
        mutate("driver.godot.animation_tree.blend_tree.node.remove", {
            "tree": "AnimationTree", "graph": "BlendGraph", "name": "RunClip",
        })

        # Multiplayer scene-replication authoring without opening sockets.
        mutate("driver.godot.multiplayer.spawner.configure", {
            "target": "Spawner", "spawn_path": ".", "spawn_limit": 8,
        })
        mutate("driver.godot.multiplayer.spawner.scene.add", {
            "target": "Spawner", "scene": "res://scenes/lab_room.tscn",
        })
        spawner = call("driver.godot.multiplayer.spawner.inspect", {"session": sid, "target": "Spawner"})
        assert "res://scenes/lab_room.tscn" in spawner["data"]["spawnable_scenes"]
        mutate("driver.godot.multiplayer.spawner.scene.remove", {
            "target": "Spawner", "scene": "res://scenes/lab_room.tscn",
        })
        mutate("driver.godot.multiplayer.spawner.scene.add", {
            "target": "Spawner", "scene": "res://scenes/lab_room.tscn",
        })
        dry_sync = call("driver.godot.multiplayer.synchronizer.inspect", {
            "session": sid, "target": "SynchronizerDry",
        })
        assert dry_sync["data"]["has_replication_config"] is False
        dry_add = call("driver.godot.multiplayer.replication.property.add", {
            "session": sid, "target": "SynchronizerDry", "path": "Player:position",
            "spawn": True, "mode": 1, "expect": st, "dry_run": True,
        })
        assert dry_add["applied"] is False
        dry_sync = call("driver.godot.multiplayer.synchronizer.inspect", {
            "session": sid, "target": "SynchronizerDry",
        })
        assert dry_sync["data"]["has_replication_config"] is False

        mutate("driver.godot.multiplayer.synchronizer.configure", {
            "target": "Synchronizer", "root_path": ".", "replication_interval": 0.1,
            "delta_interval": 0.05, "public_visibility": True, "visibility_update_mode": 0,
        })
        mutate("driver.godot.multiplayer.replication.property.add", {
            "target": "Synchronizer", "path": "Player:position", "spawn": True, "mode": 1,
        })
        mutate("driver.godot.multiplayer.replication.property.add", {
            "target": "Synchronizer", "path": "Player:rotation", "spawn": False, "mode": 2,
        })
        mutate("driver.godot.multiplayer.replication.property.configure", {
            "target": "Synchronizer", "path": "Player:position", "spawn": True, "mode": 2,
        })
        multiplayer = call("driver.godot.multiplayer.synchronizer.inspect", {
            "session": sid, "target": "Synchronizer",
        })
        assert any(row["path"] == "Player:position" for row in multiplayer["data"]["properties"])
        mutate("driver.godot.multiplayer.replication.property.remove", {
            "target": "Synchronizer", "path": "Player:rotation",
        })

        # Editor semantic state and selection. All mutations must reject stale
        # preconditions before touching editor selection or run state.
        stale_editor_stamp = dict(st)
        mutate("driver.godot.editor.selection.set", {"nodes": ["Player", "Door"]})
        selection = call("driver.godot.editor.selection.get", {"session": sid})
        assert {row["path"] for row in selection["data"]["nodes"]} == {"Player", "Door"}
        editor_state = call("driver.godot.editor.state", {"session": sid})
        assert editor_state["data"]["scene"] == "res://scenes/lab_room.tscn"
        for stale_name, stale_args in [
            ("driver.godot.editor.selection.set", {"nodes": ["Door"]}),
            ("driver.godot.editor.run.start", {"mode": "main"}),
            ("driver.godot.editor.run.stop", {}),
        ]:
            payload = dict(stale_args)
            payload.update({
                "session": sid,
                "expect": stale_editor_stamp,
                "dry_run": False,
            })
            error, _ = execute_failure(
                driver, caps, stale_name, payload, f"stale-editor-{stale_name.rsplit('.', 1)[-1]}"
            )
            assert error["code"] == "Conflict", (stale_name, error)
        editor_state = call("driver.godot.editor.state", {"session": sid})
        assert editor_state["data"]["playing"] is False
        selection = call("driver.godot.editor.selection.get", {"session": sid})
        assert {row["path"] for row in selection["data"]["nodes"]} == {"Player", "Door"}

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

        mutate("driver.godot.script.write", {
            "path": "res://scripts/semantic_service.gd",
            "source": "extends Node\nvar semantic_ready := true\n",
            "expected_sha256": "",
        })
        mutate("driver.godot.autoload.add", {
            "name": "SemanticService", "path": "res://scripts/semantic_service.gd",
        })
        autoloads = call("driver.godot.autoload.list", {"session": sid})
        assert any(row["name"] == "SemanticService" for row in autoloads["data"]["autoloads"])
        mutate("driver.godot.autoload.remove", {"name": "SemanticService"})

        for name, code in [("move_forward", 87), ("move_back", 83), ("move_left", 65), ("move_right", 68)]:
            value = call("driver.godot.input.set", {
                "session": sid, "name": name, "deadzone": 0.5,
                "events": [{"type": "key", "code": code}], "expect": st, "dry_run": False,
            })
            st = stamp(value)

        mutate("driver.godot.input.set", {
            "name": "gamepad_semantic", "deadzone": 0.2,
            "events": [
                {"type": "joy_button", "code": 0, "device": -1},
                {"type": "joy_axis", "axis": 0, "value": 1.0, "device": -1},
            ],
        })
        actions = call("driver.godot.input.list", {"session": sid})["data"]
        gamepad = next(action for action in actions if action["name"] == "gamepad_semantic")
        assert {event["type"] for event in gamepad["events"]} == {"joy_button", "joy_axis"}
        axis = next(event for event in gamepad["events"] if event["type"] == "joy_axis")
        assert axis["axis"] == 0 and abs(axis["value"] - 1.0) < 0.001

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

        mutate("driver.godot.editor.run.start", {"mode": "current", "scene": ""})
        playing = False
        for _ in range(60):
            editor_runtime = call("driver.godot.editor.state", {"session": sid})
            playing = editor_runtime["data"]["playing"]
            if playing:
                break
            time.sleep(0.05)
        assert playing, "editor.run.start did not enter playing state"
        mutate("driver.godot.editor.run.stop", {})
        time.sleep(0.1)
        assert call("driver.godot.editor.state", {"session": sid})["data"]["playing"] is False

        scene = call("driver.godot.scene.inspect", {"session": sid})
        names = {x["name"] for x in scene["data"]["nodes"]}
        assert {
            "LabRoom", "Floor", "Player", "Door", "Key", "Animations", "HUD", "Status",
            "Grid", "Rail3D", "TwoD", "Player2D", "NavRegion", "NavLink", "Area", "Joint",
            "Rail2D", "Follower",
        }.issubset(names), sorted(names)

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
