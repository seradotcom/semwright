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
    var color: Color
    var asset_colors: Array[Color] = []
    if spec.has("asset_source"):
        var meshes = marker.find_children("*", "MeshInstance3D", true, false)
        var skeletons = marker.find_children("*", "Skeleton3D", true, false)
        var animations = marker.find_children("*", "AnimationPlayer", true, false)
        checks.native_asset_mesh_count = meshes.size() == int(spec.asset_segments)
        var geometry_matches = not meshes.is_empty()
        var skin_matches = not skeletons.is_empty()
        for mesh in meshes:
            geometry_matches = geometry_matches and absf(mesh.get_aabb().size.x - float(spec.asset_width)) < 0.002
            skin_matches = skin_matches and mesh.skin != null
            var material = mesh.get_active_material(0)
            asset_colors.append(material.albedo_color if material is StandardMaterial3D else Color(0,0,0,0))
        checks.native_asset_geometry = geometry_matches
        checks.native_asset_skin = skin_matches and skeletons[0].get_bone_count() >= 2
        var animation_matches = false
        if not animations.is_empty() and not skeletons.is_empty():
            var animator = animations[0] as AnimationPlayer
            var skeleton = skeletons[0] as Skeleton3D
            var hinge = skeleton.find_bone("hinge")
            for animation_name in animator.get_animation_list():
                var clip = animator.get_animation(animation_name)
                if animation_name == "RESET" or hinge < 0 or clip.get_track_count() < 1:
                    continue
                if absf(clip.length - float(spec.asset_animation_seconds)) > 0.05:
                    continue
                animator.play(animation_name)
                animator.seek(0, true)
                animator.advance(0)
                var initial = skeleton.get_bone_pose_rotation(hinge)
                animator.seek(clip.length, true)
                animator.advance(0)
                var final = skeleton.get_bone_pose_rotation(hinge)
                animation_matches = absf(initial.angle_to(final) - float(spec.asset_rotation)) < 0.02
                animator.pause()
                if animation_matches:
                    break
        checks.native_asset_animation = animation_matches
        if meshes.is_empty():
            color = Color(0,0,0,0)
        else:
            var material = meshes[0].get_active_material(0)
            color = material.albedo_color if material is StandardMaterial3D else Color(0,0,0,0)
    else:
        color = marker.color if marker is Polygon2D else marker.material_override.albedo_color
    var expected = Color(spec.color[0],spec.color[1],spec.color[2],spec.color[3])
    var color_space = "native authored albedo"
    if spec.has("asset_source"):
        # Godot 4.7.2 gltf_document.cpp converts linear baseColorFactor to sRGB.
        # Match the imported material representation without relaxing the oracle.
        expected = expected.linear_to_srgb()
        color_space = "glTF linear baseColorFactor converted to Godot sRGB albedo"
    checks.native_material = color.is_equal_approx(expected)
    for asset_color in asset_colors:
        checks.native_material = checks.native_material and asset_color.is_equal_approx(expected)
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
        "standalone_export_input_acceptance":false,
        "material_observation":{"representation":color_space,
            "expected_rgba":[expected.r,expected.g,expected.b,expected.a],
            "observed_rgba":[color.r,color.g,color.b,color.a],
            "all_imported_rgba":asset_colors.map(func(c): return [c.r,c.g,c.b,c.a])}}
    var file = FileAccess.open(args[1], FileAccess.WRITE)
    file.store_string(JSON.stringify(report,"  "))
    file.close()
    quit(0 if ok else 2)
