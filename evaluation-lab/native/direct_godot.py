"""Competent direct-arm reusable game module for public development smoke.

No H observer is embedded in this editable game. Input actions work in the editor
and standalone export. Revisions change config on the same project.
"""
import json
from pathlib import Path

GAME = '''extends Node
var spec: Dictionary
var count: int = 0
var remaining: float = 0.0
var player: Node
var goal: int = 0
var label: Label
var stations: Array[Node] = []

func _ready() -> void:
    spec = JSON.parse_string(FileAccess.get_file_as_string("res://config.json"))
    goal = int(spec.objective_count)
    remaining = float(spec.timer_seconds)
    player = Node2D.new() if spec.dimension == "2D" else Node3D.new()
    player.name = "Player"
    add_child(player)
    if spec.dimension == "2D":
        var marker = Polygon2D.new()
        marker.polygon = PackedVector2Array([Vector2(-8,-8),Vector2(8,-8),Vector2(8,8),Vector2(-8,8)])
        marker.color = Color(spec.color[0], spec.color[1], spec.color[2], spec.color[3])
        marker.scale = Vector2.ONE * float(spec.asset_scale)
        player.add_child(marker)
    else:
        if spec.has("asset_source"):
            var asset_scene = load(str(spec.asset_source)) as PackedScene
            assert(asset_scene != null)
            var asset = asset_scene.instantiate()
            asset.name = "DeliveredAsset"
            player.add_child(asset)
        else:
            var marker = MeshInstance3D.new()
            marker.mesh = BoxMesh.new()
            marker.scale = Vector3.ONE * float(spec.asset_scale)
            var material = StandardMaterial3D.new()
            material.albedo_color = Color(spec.color[0], spec.color[1], spec.color[2], spec.color[3])
            marker.material_override = material
            player.add_child(marker)
        var camera = Camera3D.new()
        camera.position = Vector3(3, 6, 10)
        add_child(camera)
        camera.look_at(Vector3(3, 0, 0))
        camera.current = true
    for i in range(goal):
        var station = Node2D.new() if spec.dimension == "2D" else Node3D.new()
        station.name = "Station_%d" % i
        station.position.x = (i+1) * (20 if spec.dimension == "2D" else 1)
        if spec.dimension == "2D":
            var icon = Polygon2D.new()
            icon.polygon = PackedVector2Array([Vector2(0,-4),Vector2(4,0),Vector2(0,4),Vector2(-4,0)])
            icon.color = Color(0.4, 0.9, 0.3)
            station.add_child(icon)
        else:
            var icon = MeshInstance3D.new()
            icon.mesh = SphereMesh.new()
            station.add_child(icon)
        add_child(station)
        stations.append(station)
    label = Label.new()
    label.name = "Status"
    label.position = Vector2(10, 60)
    add_child(label)
    _status()

func _status() -> void:
    label.text = "Delivered %d/%d | %.1fs%s" % [count, goal, remaining, " | Complete" if count == goal else ""]

func _process(delta: float) -> void:
    if count < goal:
        remaining = maxf(0, remaining - delta)
        _status()

func _input(event: InputEvent) -> void:
    if event.is_action_pressed("restart"):
        count = 0
        remaining = float(spec.timer_seconds)
        player.position.x = 0
        for station in stations:
            station.visible = true
    elif remaining > 0 and count < goal:
        if event.is_action_pressed("right"):
            player.position.x += 20 if spec.dimension == "2D" else 1
        elif event.is_action_pressed("left"):
            player.position.x -= 20 if spec.dimension == "2D" else 1
        elif event.is_action_pressed("collect") and count < stations.size():
            if absf(player.position.x - stations[count].position.x) < 0.01:
                stations[count].visible = false
                count += 1
    _status()
'''

PROJECT = '''config_version=5
[application]
config/name="H editable delivery development game"
run/main_scene="res://main.tscn"
[display]
window/size/viewport_width=640
window/size/viewport_height=240
[rendering]
renderer/rendering_method="gl_compatibility"
'''


def apply(spec, project):
    project = Path(project)
    if spec["phase"] == "create":
        project.mkdir()
        (project / "project.godot").write_text(PROJECT)
        # Native keyboard actions. Observers inject action events independently.
        with (project / "project.godot").open("a") as stream:
            stream.write("[input]\n")
            for action, keycode in (("right", 4194321), ("left", 4194319), ("collect", 32), ("restart", 82)):
                stream.write('%s={"deadzone":0.5,"events":[Object(InputEventKey,"physical_keycode":%d)]}\n' % (action, keycode))
        (project / "main.gd").write_text(GAME)
        (project / "main.tscn").write_text('[gd_scene load_steps=2 format=3]\n[ext_resource type="Script" path="res://main.gd" id="1"]\n[node name="Delivery" type="Node"]\nscript = ExtResource("1")\n')
    else:
        if not (project / "project.godot").is_file():
            raise ValueError("Revision requires the same existing saved project")
    (project / "config.json").write_text(json.dumps(spec, indent=2)+"\n")


def export_preset(project, template):
    # The actor's declared native export configuration; no hidden scene behavior.
    options = {'custom_template/release': str(template), 'binary_format/embed_pck': True,
               'binary_format/architecture': 'x86_64'}
    text = '[preset.0]\nname="Linux"\nplatform="Linux"\nrunnable=true\nexport_filter="all_resources"\ninclude_filter="config.json"\nexclude_filter=""\n[preset.0.options]\n'
    for key, value in options.items():
        text += key+'='+json.dumps(value)+'\n'
    (Path(project) / "export_presets.cfg").write_text(text)
