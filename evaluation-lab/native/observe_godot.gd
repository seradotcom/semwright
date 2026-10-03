# Independently loaded native observer. No game behavior or source is written.
extends SceneTree
var game: Node
var checks: Dictionary = {}
var spec: Dictionary

func _initialize() -> void:
    call_deferred("observe")

func press(action: String) -> void:
    var event = InputEventAction.new()
    event.action = action
    event.pressed = true
    Input.parse_input_event(event)
    await process_frame
    event = InputEventAction.new()
    event.action = action
    event.pressed = false
    Input.parse_input_event(event)
    await process_frame

func observe() -> void:
    var args = OS.get_cmdline_user_args()
    spec = JSON.parse_string(FileAccess.get_file_as_string(args[0]))
    var scene = load("res://main.tscn") as PackedScene
    if scene == null:
        quit(2)
        return
    game = scene.instantiate()
    root.add_child(game)
    await process_frame
    var player = game.get_node("Player")
    checks.editable_native_scene = player is Node2D if spec.dimension == "2D" else player is Node3D
    checks.native_status_ui = game.get_node("Status") is Label
    var marker = player.get_child(0)
    var color: Color = marker.color if marker is Polygon2D else marker.material_override.albedo_color
    var expected = Color(spec.color[0],spec.color[1],spec.color[2],spec.color[3])
    checks.native_material = color.is_equal_approx(expected)
    checks.native_asset_scale = absf(marker.scale.x - float(spec.asset_scale)) < 0.001
    var timer_start = float(game.remaining)
    await create_timer(0.12).timeout
    checks.timer_active = float(game.remaining) < timer_start and float(game.remaining) > 0
    await press("collect")
    checks.no_pickup_at_wrong_position = int(game.count) == 0
    await press("right")
    checks.input_moves_player = player.position.x > 0
    await press("left")
    checks.reverse_input = absf(player.position.x) < 0.001
    for i in range(int(spec.objective_count)):
        await press("right")
        await press("collect")
    checks.objective_from_input_events = int(game.count) == int(spec.objective_count)
    checks.completion_ui = "Complete" in game.get_node("Status").text
    await press("restart")
    checks.restart_state = int(game.count) == 0 and absf(player.position.x) < 0.001
    checks.restart_ui = "Complete" not in game.get_node("Status").text
    checks.restart_timer = float(game.remaining) > float(spec.timer_seconds) - 1
    var ok = true
    for value in checks.values():
        ok = ok and bool(value)
    var report = {"schema_version":1,"oracle":"H-native-godot-input-v1","native":true,
        "outcome":"PASS" if ok else "FAIL","checks":checks,
        "input_origin":"independent Input.parse_input_event; not calls to game methods",
        "standalone_export_input_acceptance":false}
    var file = FileAccess.open(args[1], FileAccess.WRITE)
    file.store_string(JSON.stringify(report,"  "))
    file.close()
    quit(0 if ok else 2)
