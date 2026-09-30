extends SceneTree
# Fixed Semwright backend probe, executed only in an authorized disposable copy.
# This file observes/persists native objects. It never creates the target game,
# rewrites gameplay, or belongs in a normal exported package.
const VERSION: int = 1
const MAX_NODES: int = 4096
const MAX_RESOURCES: int = 4096
const MAX_TRACKS: int = 2048
const MAX_KEYS: int = 16384
const MAX_BYTES: int = 8388608
var _request: Dictionary = {}
var _output: String = ""
var _failures: Array[String] = []
var _resources: Array = []
var _resource_bindings: Dictionary = {}
var _unknown: Array[String] = []
var _tracks: int = 0
var _keys: int = 0
var _connection_count: int = 0
var _root_scene: Node
var _scene_paths: Dictionary = {}

func _initialize() -> void:
    _run.call_deferred()

func _fail(reason: String) -> bool:
    if _failures.size() < 128 and not _failures.has(reason):
        _failures.append(reason)
        push_error("SEMWRIGHT_NATIVE_OBSERVER: " + reason)
    return false

func _note(reason: String) -> void:
    if not _unknown.has(reason):
        if _unknown.size() >= 128: _fail("unknown_coverage_budget")
        else: _unknown.append(reason.left(512))

func _id(value: String) -> bool:
    if value.is_empty() or value.length() > 48: return false
    if value[0] < "a" or value[0] > "z": return false
    for character in value:
        if not ((character >= "a" and character <= "z") or (character >= "0" and character <= "9") or character == "_"):
            return false
    return true

func _read_request() -> bool:
    var arguments: PackedStringArray = OS.get_cmdline_user_args()
    if arguments.size() != 4 or arguments[0] != "--request" or arguments[2] != "--output":
        return _fail("fixed_request_output_arguments_required")
    var input_path: String = arguments[1]
    _output = arguments[3]
    if not input_path.is_absolute_path() or not _output.is_absolute_path() or FileAccess.file_exists(_output):
        return _fail("private_request_output_paths_required")
    var file: FileAccess = FileAccess.open(input_path, FileAccess.READ)
    if file == null or file.get_length() > 32768: return _fail("request_file_bound")
    var parser: JSON = JSON.new()
    if parser.parse(file.get_as_text()) != OK or not parser.data is Dictionary: return _fail("request_json")
    _request = parser.data
    var names: Array = ["version", "nonce", "source_fingerprint", "mode", "scene", "ticks", "inputs", "checkpoints", "variables", "capture"]
    if _request.size() != names.size(): return _fail("unknown_request_field")
    for name in names:
        if not _request.has(name): return _fail("missing_request_field")
    if _request.version != VERSION or not _request.nonce is String or not _request.source_fingerprint is String:
        return _fail("request_identity_types")
    if _request.nonce.length() < 16 or _request.nonce.length() > 80 or _request.source_fingerprint.length() != 64:
        return _fail("request_identity_bounds")
    if not _request.mode in ["inspect", "save_candidate", "reopen_candidate", "play"]:
        return _fail("unsupported_probe_mode")
    if not _request.scene is String or not _request.scene.begins_with("res://scenes/") or not _request.scene.ends_with(".tscn"):
        return _fail("managed_scene_locator_required")
    if not _id(_request.scene.trim_prefix("res://scenes/").trim_suffix(".tscn")):
        return _fail("invalid_managed_scene_locator")
    if not _request.inputs is Array or not _request.checkpoints is Array or not _request.variables is Array:
        return _fail("request_collection_types")
    if _request.inputs.size() > 256 or _request.checkpoints.size() > 32 or _request.variables.size() > 64:
        return _fail("request_collection_bounds")
    if not (_request.ticks is float or _request.ticks is int) or not _request.capture is bool:
        return _fail("request_tick_capture_type")
    if not is_finite(float(_request.ticks)) or _request.ticks != int(_request.ticks) or _request.ticks < 0 or _request.ticks > 3600:
        return _fail("request_tick_bound")
    if _request.mode != "play" and (_request.ticks != 0 or not _request.inputs.is_empty() or not _request.checkpoints.is_empty() or _request.capture):
        return _fail("nonplay_request_may_not_inject_input")
    if _request.mode == "play" and (_request.ticks < 1 or _request.checkpoints.is_empty()):
        return _fail("play_requires_ticks_and_observations")
    var variables: Dictionary = {}
    for variable in _request.variables:
        if not variable is String or not _id(variable) or variables.has(variable): return _fail("variable_scope")
        variables[variable] = true
    var previous: int = 0
    var pressed: Dictionary = {}
    for step in _request.inputs:
        if not step is Dictionary or step.size() != 3 or not step.has_all(["tick", "action", "pressed"]):
            return _fail("input_event_fields")
        if not step.action is String or not _id(step.action) or not InputMap.has_action(step.action) or not step.pressed is bool:
            return _fail("input_action_not_declared")
        if not (step.tick is int or step.tick is float) or not is_finite(float(step.tick)) or step.tick != int(step.tick):
            return _fail("input_tick_type")
        if step.tick < 1 or step.tick < previous or step.tick > _request.ticks or pressed.get(step.action, false) == step.pressed:
            return _fail("input_event_order_or_duplicate_state")
        previous = int(step.tick)
        pressed[step.action] = step.pressed
    for held in pressed.values():
        if held: return _fail("unreleased_input_action")
    previous = 0
    for tick in _request.checkpoints:
        if not (tick is int or tick is float) or not is_finite(float(tick)) or tick != int(tick): return _fail("checkpoint_type")
        if tick <= previous or tick > _request.ticks: return _fail("checkpoint_order")
        previous = int(tick)
    var version: Dictionary = Engine.get_version_info()
    if version.major != 4 or version.minor != 7 or version.patch != 2 or version.status != "stable":
        return _fail("pinned_engine_version_mismatch")
    return true

func _engine_version() -> String:
    var version: Dictionary = Engine.get_version_info()
    return "%d.%d.%d.%s.%s.%s" % [version.major, version.minor, version.patch, version.status, version.build, version.hash]

func _run() -> void:
    if not _read_request():
        quit(2)
        return
    ResourceLoader.set_abort_on_missing_resources(false)
    var scene_path: String = _request.scene
    if _request.mode == "reopen_candidate": scene_path = scene_path.replace("res://scenes/", "res://__sw_saved/")
    var source_hash: String = FileAccess.get_sha256(scene_path)
    if source_hash.is_empty():
        _fail("scene_source_not_readable")
        quit(2)
        return
    var packed: PackedScene = ResourceLoader.load(scene_path, "PackedScene", ResourceLoader.CACHE_MODE_IGNORE_DEEP) as PackedScene
    if packed == null or not packed.can_instantiate():
        _fail("native_scene_parse_or_instantiation_failed")
        quit(2)
        return
    _root_scene = packed.instantiate(PackedScene.GEN_EDIT_STATE_DISABLED)
    if _root_scene == null:
        _fail("native_scene_instance_missing")
        quit(2)
        return
    var authored: Dictionary = _projection(_root_scene)
    var report: Dictionary = {"version": VERSION, "nonce": _request.nonce, "source_fingerprint": _request.source_fingerprint,
        "mode": _request.mode, "engine_version": _engine_version(), "process_id": str(OS.get_process_id()),
        "loaded_scene": scene_path, "loaded_scene_sha256": source_hash, "candidate_sha256": null,
        "authored": authored, "live": null, "frames": [], "dependencies": [], "dependency_complete": false,
        "inputs_delivered": 0, "elapsed_physics_frames": 0, "failures": []}
    if _request.mode == "save_candidate" and _failures.is_empty():
        var directory: String = ProjectSettings.globalize_path("res://__sw_saved")
        var candidate_path: String = _request.scene.replace("res://scenes/", "res://__sw_saved/")
        if FileAccess.file_exists(candidate_path): _fail("candidate_already_exists")
        elif DirAccess.make_dir_recursive_absolute(directory) != OK: _fail("candidate_directory_creation_failed")
        else:
            var candidate: PackedScene = PackedScene.new()
            if candidate.pack(_root_scene) != OK: _fail("native_packedscene_pack_failed")
            elif ResourceSaver.save(candidate, candidate_path, 0) != OK: _fail("native_candidate_save_failed")
            else:
                var candidate_hash: String = FileAccess.get_sha256(candidate_path)
                if candidate_hash.is_empty(): _fail("native_candidate_digest_missing")
                else: report.candidate_sha256 = candidate_hash
    if _request.mode == "play" and _failures.is_empty():
        root.add_child(_root_scene)
        current_scene = _root_scene
        report.live = _projection(_root_scene)
        var started: int = Engine.get_physics_frames()
        var input_index: int = 0
        for tick in range(1, int(_request.ticks) + 1):
            while input_index < _request.inputs.size() and int(_request.inputs[input_index].tick) == tick:
                var step: Dictionary = _request.inputs[input_index]
                var event: InputEventAction = InputEventAction.new()
                event.action = step.action
                event.pressed = step.pressed
                event.strength = 1.0 if step.pressed else 0.0
                Input.parse_input_event(event)
                Input.flush_buffered_events()
                input_index += 1
            await physics_frame
            await process_frame
            if not is_instance_valid(current_scene):
                _fail("runtime_scene_missing")
                break
            for checkpoint in _request.checkpoints:
                if int(checkpoint) == tick:
                    report.frames.append(_runtime_frame(tick))
                    break
            if not _failures.is_empty(): break
        report.inputs_delivered = input_index
        report.elapsed_physics_frames = Engine.get_physics_frames() - started
    var dependency_result: Dictionary = _dependencies(scene_path)
    report.dependencies = dependency_result.edges
    report.dependency_complete = dependency_result.complete
    if FileAccess.get_sha256(scene_path) != source_hash: _fail("source_changed_during_native_observation")
    report.failures = _failures
    _finish(report)

func _finish(report: Dictionary) -> void:
    var serialized: String = JSON.stringify(report, "", true, true)
    var bytes: PackedByteArray = serialized.to_utf8_buffer()
    if bytes.size() > MAX_BYTES:
        _fail("native_observation_byte_budget")
        quit(2)
        return
    var file: FileAccess = FileAccess.open(_output, FileAccess.WRITE)
    if file == null:
        _fail("native_receipt_write_failed")
        quit(2)
        return
    file.store_buffer(bytes)
    file.flush()
    if file.get_error() != OK: _fail("native_receipt_flush_failed")
    file.close()
    if is_instance_valid(current_scene):
        var scene: Node = current_scene
        current_scene = null
        scene.free()
    elif is_instance_valid(_root_scene): _root_scene.free()
    quit(0 if _failures.is_empty() else 2)

func _all_nodes(scene: Node) -> Array[Node]:
    var pending: Array[Node] = [scene]
    var nodes: Array[Node] = []
    while not pending.is_empty():
        var node: Node = pending.pop_back()
        if nodes.size() >= MAX_NODES:
            _fail("native_node_budget")
            return nodes
        nodes.append(node)
        for child in node.get_children(true): pending.append(child)
    return nodes

func _projection(scene: Node) -> Dictionary:
    _resources = []
    _resource_bindings = {}
    _unknown = []
    _tracks = 0
    _keys = 0
    _connection_count = 0
    _scene_paths = {}
    var nodes: Array[Node] = _all_nodes(scene)
    for node in nodes: _scene_paths[node.get_instance_id()] = str(scene.get_path_to(node))
    nodes.sort_custom(func(a: Node, b: Node) -> bool: return _scene_paths[a.get_instance_id()] < _scene_paths[b.get_instance_id()])
    var records: Array = []
    var animations: Array = []
    var connections: Array = []
    for node in nodes:
        if not _failures.is_empty(): break
        var path: String = _scene_paths[node.get_instance_id()]
        var groups: Array[String] = []
        for group in node.get_groups():
            if not str(group).begins_with("_"): groups.append(str(group))
        groups.sort()
        if groups.size() > 64: _fail("native_group_budget")
        var parent: Variant = null
        if node != scene and is_instance_valid(node.get_parent()): parent = _scene_paths.get(node.get_parent().get_instance_id())
        var owner_path: Variant = null
        if is_instance_valid(node.owner):
            owner_path = _scene_paths.get(node.owner.get_instance_id())
            if owner_path == null: _note("owner_outside_scene:" + path)
        var logical_id: Variant = node.get_meta("semwright_logical_id", null)
        var logical_key: Variant = node.get_meta("semwright_logical_key", null)
        if logical_id != null and not logical_id is String: _fail("logical_identity_metadata_type")
        if logical_key != null and not logical_key is String: _fail("logical_key_metadata_type")
        records.append({"path": path, "class": node.get_class(), "instance_id": str(node.get_instance_id()),
            "parent": parent, "owner": owner_path, "scene_file": node.scene_file_path,
            "logical_id": logical_id, "logical_key": logical_key, "groups": groups, "properties": _node_properties(node, path)})
        if node is AnimationPlayer: animations.append_array(_animations(node, path))
        connections.append_array(_connections(node, path))
    return {"nodes": records, "resources": _resources.duplicate(true), "animations": animations,
        "connections": connections, "unknown": _unknown.duplicate()}

func _node_properties(node: Node, path: String) -> Dictionary:
    var values: Dictionary = {}
    if node is Node2D:
        values.position = _value(node.position, path + ":position")
        values.rotation = _value(node.rotation, path + ":rotation")
        values.scale = _value(node.scale, path + ":scale")
        values.transform = _value(node.transform, path + ":transform")
    if node is Node3D:
        values.transform = _value(node.transform, path + ":transform")
        values.visible = _value(node.visible, path + ":visible")
    if node is CanvasItem:
        values.visible = _value(node.visible, path + ":visible")
        values.modulate = _value(node.modulate, path + ":modulate")
    if node is CollisionObject2D or node is CollisionObject3D:
        values.collision_layer = _value(node.collision_layer, path + ":collision_layer")
        values.collision_mask = _value(node.collision_mask, path + ":collision_mask")
    if node is CollisionShape2D or node is CollisionShape3D:
        values.shape = _value(node.shape, path + ":shape")
        values.disabled = _value(node.disabled, path + ":disabled")
    if node is CharacterBody2D or node is CharacterBody3D:
        values.velocity = _value(node.velocity, path + ":velocity")
        values.up_direction = _value(node.up_direction, path + ":up_direction")
    if node is Area2D or node is Area3D:
        values.monitoring = _value(node.monitoring, path + ":monitoring")
        values.monitorable = _value(node.monitorable, path + ":monitorable")
    if node is Polygon2D:
        values.polygon = _value(node.polygon, path + ":polygon")
        values.color = _value(node.color, path + ":color")
    if node is MeshInstance3D:
        values.mesh = _value(node.mesh, path + ":mesh")
        values.material_override = _value(node.material_override, path + ":material_override")
        values.skeleton = _value(node.skeleton, path + ":skeleton")
        if node.mesh != null:
            if node.mesh.get_surface_count() > 64: _fail("mesh_surface_readback_budget")
            else:
                for index in range(node.mesh.get_surface_count()):
                    values["surface_override_" + str(index)] = _value(node.get_surface_override_material(index), path + ":surface_override_" + str(index))
    if node is Sprite2D: values.texture = _value(node.texture, path + ":texture")
    if node is Camera2D:
        values.enabled = _value(node.enabled, path + ":enabled")
        values.zoom = _value(node.zoom, path + ":zoom")
    if node is Camera3D:
        values.fov = _value(node.fov, path + ":fov")
        values.current = _value(node.current, path + ":current")
        values.near = _value(node.near, path + ":near")
        values.far = _value(node.far, path + ":far")
    if node is Light3D:
        values.light_color = _value(node.light_color, path + ":light_color")
        values.light_energy = _value(node.light_energy, path + ":light_energy")
    if node is Label:
        values.text = _value(node.text, path + ":text")
        values.font_size = _value(node.get_theme_font_size("font_size"), path + ":font_size")
    if node is Control:
        values.position = _value(node.position, path + ":position")
        values.size = _value(node.size, path + ":size")
        values.custom_minimum_size = _value(node.custom_minimum_size, path + ":custom_minimum_size")
        values.mouse_filter = _value(node.mouse_filter, path + ":mouse_filter")
    if node is CanvasLayer:
        values.layer = _value(node.layer, path + ":layer")
        values.transform = _value(node.transform, path + ":transform")
    if node is Timer:
        values.wait_time = _value(node.wait_time, path + ":wait_time")
        values.one_shot = _value(node.one_shot, path + ":one_shot")
        values.autostart = _value(node.autostart, path + ":autostart")
        values.process_callback = _value(node.process_callback, path + ":process_callback")
    if node is AudioStreamPlayer or node is AudioStreamPlayer2D or node is AudioStreamPlayer3D:
        values.stream = _value(node.get("stream"), path + ":stream")
        values.bus = _value(node.get("bus"), path + ":bus")
        values.volume_db = _value(node.get("volume_db"), path + ":volume_db")
    if node is AnimationTree:
        values.active = _value(node.active, path + ":animation_tree_active")
        values.anim_player = _value(node.anim_player, path + ":animation_tree_player")
        values.tree_root = _value(node.tree_root, path + ":animation_tree_root")
        if node.tree_root is AnimationNodeStateMachine:
            var playback: AnimationNodeStateMachinePlayback = node.get("parameters/playback") as AnimationNodeStateMachinePlayback
            if playback == null:
                _fail("animation_state_machine_playback_missing:" + path)
            else:
                values.current_state = _value(playback.get_current_node(), path + ":animation_current_state")
        elif node.tree_root is AnimationNodeBlendSpace1D:
            values.blend_position = _value(node.get("parameters/blend_position"), path + ":animation_blend_position")
    if node is Skeleton3D:
        values.bone_count = _value(node.get_bone_count(), path + ":bone_count")
        if node.get_bone_count() > 32: _note("skeleton_rest_scope_limit:" + path)
        else:
            for index in range(node.get_bone_count()):
                values["bone_name_" + str(index)] = _value(node.get_bone_name(index), path + ":bone_name_" + str(index))
                values["bone_parent_" + str(index)] = _value(node.get_bone_parent(index), path + ":bone_parent_" + str(index))
                values["bone_rest_" + str(index)] = _value(node.get_bone_rest(index), path + ":bone_rest_" + str(index))
    if node is NavigationRegion3D:
        values.navigation_mesh = _value(node.navigation_mesh, path + ":navigation_mesh")
        values.navigation_layers = _value(node.navigation_layers, path + ":navigation_layers")
    if node is NavigationRegion2D:
        values.navigation_polygon = _value(node.navigation_polygon, path + ":navigation_polygon")
        values.navigation_layers = _value(node.navigation_layers, path + ":navigation_layers")
    if node.get_script() != null: values.script = _value(node.get_script(), path + ":script")
    if values.size() > 128: _fail("native_node_property_budget")
    return values

func _resource_reference(resource: Resource) -> Dictionary:
    var uid: Variant = null
    var path: String = resource.resource_path
    var source: String = path.get_slice("::", 0)
    if source.begins_with("res://") and FileAccess.file_exists(source):
        var native_uid: int = ResourceLoader.get_resource_uid(source)
        if native_uid != -1: uid = ResourceUID.id_to_text(native_uid)
    return {"class": resource.get_class(), "path": path, "uid": uid,
        "instance_id": str(resource.get_instance_id()), "local_to_scene": resource.resource_local_to_scene}

func _resource(resource: Resource, binding: String, depth: int) -> Dictionary:
    var reference: Dictionary = _resource_reference(resource)
    if _resource_bindings.has(binding): return reference
    if _resources.size() >= MAX_RESOURCES or depth > 8:
        _fail("native_resource_scope_budget")
        return reference
    _resource_bindings[binding] = true
    var record: Dictionary = {"binding": binding, "resource": reference, "properties": {}}
    _resources.append(record)
    record.properties = _resource_properties(resource, binding, depth + 1)
    return reference

func _resource_properties(resource: Resource, binding: String, depth: int) -> Dictionary:
    var values: Dictionary = {}
    var names: Array[String] = []
    if resource is RectangleShape2D or resource is BoxShape3D or resource is BoxMesh: names = ["size"]
    elif resource is CircleShape2D or resource is SphereShape3D: names = ["radius"]
    elif resource is CapsuleShape2D or resource is CapsuleShape3D or resource is CapsuleMesh or resource is SphereMesh:
        names = ["radius", "height"]
    elif resource is StandardMaterial3D:
        names = ["albedo_color", "metallic", "roughness", "transparency", "shading_mode", "albedo_texture", "normal_enabled", "normal_texture"]
    elif resource is PhysicsMaterial: names = ["friction", "bounce", "rough", "absorbent"]
    elif resource is ShaderMaterial:
        names = ["shader"]
        _note("shader_material_parameters_not_enumerated:" + binding)
    elif resource is AnimationNodeAnimation:
        values.animation = _value(resource.animation, binding + ":animation", depth)
    elif resource is AnimationNodeStateMachineTransition:
        values.switch_mode = _value(int(resource.switch_mode), binding + ":switch_mode", depth)
        values.advance_mode = _value(int(resource.advance_mode), binding + ":advance_mode", depth)
        values.xfade_time = _value(resource.xfade_time, binding + ":xfade_time", depth)
        values.reset = _value(resource.reset, binding + ":reset", depth)
        if not str(resource.advance_condition).is_empty() or not resource.advance_expression.is_empty():
            _fail("animation_transition_condition_or_expression:" + binding)
    elif resource is AnimationNodeStateMachine:
        var state_names: Array[String] = []
        for info in resource.get_property_list():
            var dynamic_name: String = str(info.get("name", ""))
            if dynamic_name.begins_with("states/") and dynamic_name.ends_with("/node"):
                var state_name: String = dynamic_name.get_slice("/", 1)
                if state_name != "Start" and state_name != "End" and not state_names.has(state_name):
                    state_names.append(state_name)
        state_names.sort()
        if state_names.size() > 16: _fail("animation_state_machine_state_budget:" + binding)
        else:
            values.state_count = _value(state_names.size(), binding + ":state_count", depth)
            for index in range(state_names.size()):
                var state_name: String = state_names[index]
                values["state_" + str(index) + "_name"] = _value(state_name, binding + ":state_name", depth)
                values["state_" + str(index) + "_position"] = _value(resource.get_node_position(state_name), binding + ":state_position", depth)
                var state_node: AnimationNode = resource.get_node(state_name)
                values["state_" + str(index) + "_node"] = _value(state_node, binding + ":state_node", depth)
        var transition_count: int = resource.get_transition_count()
        if transition_count > 24: _fail("animation_state_machine_transition_budget:" + binding)
        else:
            values.transition_count = _value(transition_count, binding + ":transition_count", depth)
            for index in range(transition_count):
                values["transition_" + str(index) + "_from"] = _value(resource.get_transition_from(index), binding + ":transition_from", depth)
                values["transition_" + str(index) + "_to"] = _value(resource.get_transition_to(index), binding + ":transition_to", depth)
                values["transition_" + str(index) + "_resource"] = _value(resource.get_transition(index), binding + ":transition_resource", depth)
    elif resource is AnimationNodeBlendSpace1D:
        values.min_space = _value(resource.min_space, binding + ":min_space", depth)
        values.max_space = _value(resource.max_space, binding + ":max_space", depth)
        values.sync_mode = _value(int(resource.sync_mode), binding + ":sync_mode", depth)
        values.cyclic_length = _value(resource.cyclic_length, binding + ":cyclic_length", depth)
        var point_count: int = resource.get_blend_point_count()
        if point_count > 16: _fail("animation_blend_point_budget:" + binding)
        else:
            values.point_count = _value(point_count, binding + ":point_count", depth)
            for index in range(point_count):
                values["point_" + str(index) + "_name"] = _value(resource.get_blend_point_name(index), binding + ":point_name", depth)
                values["point_" + str(index) + "_position"] = _value(resource.get_blend_point_position(index), binding + ":point_position", depth)
                values["point_" + str(index) + "_node"] = _value(resource.get_blend_point_node(index), binding + ":point_node", depth)
    if resource is PrimitiveMesh: names.append("material")
    for name in names: values[name] = _value(resource.get(name), binding + ":" + name, depth)
    if resource is Mesh:
        values.surface_count = _value(resource.get_surface_count(), binding + ":surface_count")
        var bounds: AABB = resource.get_aabb()
        values.bounds_position = _value(bounds.position, binding + ":bounds_position")
        values.bounds_size = _value(bounds.size, binding + ":bounds_size")
        if resource.get_surface_count() > 64: _fail("mesh_resource_surface_budget")
        else:
            for index in range(resource.get_surface_count()):
                values["surface_material_" + str(index)] = _value(resource.surface_get_material(index), binding + ":surface_material_" + str(index), depth)
                values["surface_vertices_" + str(index)] = _value(resource.surface_get_array_len(index), binding + ":surface_vertices_" + str(index))
                values["surface_indices_" + str(index)] = _value(resource.surface_get_array_index_len(index), binding + ":surface_indices_" + str(index))
        if resource.get_surface_count() > 32: _fail("mesh_property_projection_budget")
    if resource is Texture2D:
        values.width = _value(resource.get_width(), binding + ":width")
        values.height = _value(resource.get_height(), binding + ":height")
    if resource is AudioStream: values.length_seconds = _value(resource.get_length(), binding + ":length_seconds")
    var source: String = resource.resource_path.get_slice("::", 0)
    if source.begins_with("res://") and FileAccess.file_exists(source):
        values.source_sha256 = _value(FileAccess.get_sha256(source), binding + ":source_sha256")
    if values.is_empty(): _note("resource_properties_not_enumerated:" + resource.get_class() + ":" + binding)
    if values.size() > 128: _fail("resource_property_budget")
    return values

func _value(value: Variant, binding: String, depth: int = 0) -> Dictionary:
    if value == null: return {"type": "null"}
    if value is bool: return {"type": "bool", "value": value}
    if value is int: return {"type": "int", "value": str(value)}
    if value is float:
        if not is_finite(value):
            _fail("nonfinite_native_scalar:" + binding)
            return {"type": "unsupported", "value": "nonfinite_scalar"}
        return {"type": "float", "value": value}
    if value is String or value is StringName or value is NodePath:
        var text: String = str(value)
        if text.to_utf8_buffer().size() > 8192:
            _fail("native_string_budget:" + binding)
            return {"type": "unsupported", "value": "string_budget"}
        return {"type": "text", "value": text}
    if value is Vector2: return {"type": "vector2", "value": [value.x, value.y]}
    if value is Vector2i: return {"type": "vector2", "value": [value.x, value.y]}
    if value is Vector3: return {"type": "vector3", "value": [value.x, value.y, value.z]}
    if value is Vector3i: return {"type": "vector3", "value": [value.x, value.y, value.z]}
    if value is Quaternion: return {"type": "quaternion", "value": [value.x, value.y, value.z, value.w]}
    if value is Color: return {"type": "color", "value": [value.r, value.g, value.b, value.a]}
    if value is Transform2D:
        return {"type": "transform2", "value": [value.x.x, value.x.y, value.y.x, value.y.y, value.origin.x, value.origin.y]}
    if value is Transform3D:
        return {"type": "transform3", "value": [value.basis.x.x, value.basis.x.y, value.basis.x.z,
            value.basis.y.x, value.basis.y.y, value.basis.y.z, value.basis.z.x, value.basis.z.y, value.basis.z.z,
            value.origin.x, value.origin.y, value.origin.z]}
    if value is Resource: return {"type": "resource", "value": _resource(value, binding, depth)}
    if value is PackedVector2Array or value is PackedVector3Array or value is PackedFloat32Array or value is PackedFloat64Array or value is PackedInt32Array or value is PackedInt64Array:
        if value.size() > 128:
            _fail("native_numeric_collection_budget:" + binding)
            return {"type": "unsupported", "value": "numeric_collection_budget"}
        var numbers: Array = []
        for item in value:
            if item is Vector2: numbers.append_array([item.x, item.y])
            elif item is Vector3: numbers.append_array([item.x, item.y, item.z])
            else: numbers.append(item)
        if numbers.size() > 256:
            _fail("native_numeric_component_budget:" + binding)
            return {"type": "unsupported", "value": "numeric_component_budget"}
        for number in numbers:
            if not is_finite(float(number)): _fail("nonfinite_native_numeric_collection:" + binding)
        return {"type": "numbers", "value": numbers}
    var reason: String = "native_type_" + str(typeof(value))
    _note(reason + ":" + binding)
    return {"type": "unsupported", "value": reason}

func _animations(player: AnimationPlayer, path: String) -> Array:
    var records: Array = []
    var libraries: PackedStringArray = player.get_animation_library_list()
    libraries.sort()
    for library_name in libraries:
        var library: AnimationLibrary = player.get_animation_library(library_name)
        var names: PackedStringArray = library.get_animation_list()
        names.sort()
        if names.size() > 128: _fail("animation_library_collection_budget")
        if not _failures.is_empty(): return records
        for animation_name in names:
            if records.size() >= 128:
                _fail("animation_collection_budget")
                return records
            var animation: Animation = library.get_animation(animation_name)
            if animation == null:
                _fail("animation_resource_missing")
                return records
            var track_count: int = animation.get_track_count()
            _tracks += track_count
            if _tracks > MAX_TRACKS:
                _fail("animation_track_collection_budget")
                return records
            var tracks: Array = []
            for track_index in range(track_count):
                var key_count: int = animation.track_get_key_count(track_index)
                _keys += key_count
                if _keys > MAX_KEYS:
                    _fail("animation_key_collection_budget")
                    return records
                var keys: Array = []
                for key_index in range(key_count):
                    keys.append({
                        "time": animation.track_get_key_time(track_index, key_index),
                        "transition": animation.track_get_key_transition(track_index, key_index),
                        "value": _value(animation.track_get_key_value(track_index, key_index),
                            path + ":animation:" + str(animation_name) + ":track:" + str(track_index) + ":key:" + str(key_index)),
                    })
                tracks.append({
                    "index": track_index,
                    "track_type": int(animation.track_get_type(track_index)),
                    "path": str(animation.track_get_path(track_index)),
                    "enabled": animation.track_is_enabled(track_index),
                    "interpolation": int(animation.track_get_interpolation_type(track_index)),
                    "imported": animation.track_is_imported(track_index),
                    "key_count": key_count,
                    "keys": keys,
                })
            records.append({
                "player": path,
                "library": str(library_name),
                "name": str(animation_name),
                "root": str(player.root_node),
                "length": animation.length,
                "loop_mode": int(animation.loop_mode),
                "resource": _resource_reference(animation),
                "track_count": track_count,
                "tracks": tracks,
            })
    return records

func _connections(node: Node, path: String) -> Array:
    var records: Array = []
    var signals: Array = node.get_signal_list()
    signals.sort_custom(func(a: Dictionary, b: Dictionary) -> bool: return str(a.get("name", "")) < str(b.get("name", "")))
    for signal_info in signals:
        var signal_name: StringName = signal_info.get("name", &"")
        if signal_name.is_empty(): continue
        var connections: Array = node.get_signal_connection_list(signal_name)
        for connection in connections:
            _connection_count += 1
            if _connection_count > 4096:
                _fail("native_signal_connection_budget")
                return records
            var callable: Callable = connection.get("callable", Callable())
            if not callable.is_valid():
                _note("invalid_signal_callable:" + path + ":" + str(signal_name))
                continue
            var target: Object = callable.get_object()
            if not target is Node or not _scene_paths.has(target.get_instance_id()):
                _note("signal_target_outside_scene:" + path + ":" + str(signal_name))
                continue
            var bound: Array = callable.get_bound_arguments()
            if bound.size() > 16:
                _fail("signal_bound_argument_budget")
                return records
            var encoded: Array = []
            for index in range(bound.size()):
                encoded.append(_value(bound[index], path + ":signal:" + str(signal_name) + ":bind:" + str(index)))
            records.append({
                "source": path,
                "signal": str(signal_name),
                "target": _scene_paths[target.get_instance_id()],
                "method": str(callable.get_method()),
                "flags": int(connection.get("flags", 0)),
                "binds": encoded,
            })
    records.sort_custom(func(a: Dictionary, b: Dictionary) -> bool:
        var left: String = "%s\u0000%s\u0000%s\u0000%s" % [a.source, a.signal, a.target, a.method]
        var right: String = "%s\u0000%s\u0000%s\u0000%s" % [b.source, b.signal, b.target, b.method]
        return left < right)
    return records

func _dependency_parts(raw: String) -> Dictionary:
    if raw.contains("::"):
        var uid_text: String = raw.get_slice("::", 0)
        var path: String = raw.get_slice("::", 2)
        var uid: Variant = uid_text if uid_text.begins_with("uid://") else null
        return {"uid": uid, "path": path, "valid": not path.is_empty()}
    return {"uid": null, "path": raw, "valid": not raw.is_empty()}

func _dependencies(root_source: String) -> Dictionary:
    var pending: Array[String] = [root_source]
    var seen_sources: Dictionary = {}
    var edge_keys: Dictionary = {}
    var edges: Array = []
    var complete: bool = true
    while not pending.is_empty():
        var source: String = pending.pop_front()
        if seen_sources.has(source): continue
        seen_sources[source] = true
        if seen_sources.size() > 4096:
            _fail("native_dependency_source_budget")
            complete = false
            break
        if not source.begins_with("res://") or not ResourceLoader.exists(source):
            _note("dependency_source_unreadable:" + source)
            complete = false
            continue
        var raw_dependencies: PackedStringArray = ResourceLoader.get_dependencies(source)
        for raw in raw_dependencies:
            var parts: Dictionary = _dependency_parts(str(raw))
            var path: String = str(parts.path)
            if not bool(parts.valid) or not path.begins_with("res://") or path.contains(".."):
                _note("dependency_locator_unresolved:" + str(raw))
                complete = false
                continue
            var key: String = source + "\u0000" + path
            if edge_keys.has(key): continue
            if edges.size() >= 4096:
                _fail("native_dependency_edge_budget")
                complete = false
                break
            edge_keys[key] = true
            var exists: bool = FileAccess.file_exists(path)
            var sha: Variant = null
            if exists:
                var digest: String = FileAccess.get_sha256(path)
                if digest.length() == 64: sha = digest
                else:
                    _note("dependency_digest_failed:" + path)
                    complete = false
            else:
                _note("dependency_missing:" + path)
                complete = false
            edges.append({"source": source, "path": path, "uid": parts.uid,
                "sha256": sha, "exists": exists})
            if exists and not seen_sources.has(path): pending.append(path)
        if not _failures.is_empty(): break
    edges.sort_custom(func(a: Dictionary, b: Dictionary) -> bool:
        var left: String = str(a.source) + "\u0000" + str(a.path)
        var right: String = str(b.source) + "\u0000" + str(b.path)
        return left < right)
    return {"edges": edges, "complete": complete and _failures.is_empty()}

func _has_property(object: Object, property: String) -> bool:
    for info in object.get_property_list():
        if str(info.get("name", "")) == property: return true
    return false

func _capture_digest() -> Variant:
    var viewport: Viewport = root.get_viewport()
    if viewport == null or viewport.get_texture() == null:
        _fail("runtime_capture_viewport_missing")
        return null
    var image: Image = viewport.get_texture().get_image()
    if image == null or image.is_empty():
        _fail("runtime_capture_image_missing")
        return null
    var png: PackedByteArray = image.save_png_to_buffer()
    if png.is_empty() or png.size() > 16 * 1024 * 1024:
        _fail("runtime_capture_png_budget")
        return null
    var hash: HashingContext = HashingContext.new()
    if hash.start(HashingContext.HASH_SHA256) != OK or hash.update(png) != OK:
        _fail("runtime_capture_hash_failed")
        return null
    return hash.finish().hex_encode()

func _runtime_frame(requested_tick: int) -> Dictionary:
    var scene: Node = current_scene
    var variables: Dictionary = {}
    var root_properties: Dictionary = {}
    for info in scene.get_property_list():
        root_properties[str(info.get("name", ""))] = true
    for variable in _request.variables:
        var native_name: String = "v_" + str(variable)
        if not root_properties.has(native_name):
            _fail("runtime_variable_missing:" + str(variable))
            continue
        variables[str(variable)] = _value(scene.get(native_name), "runtime:variable:" + str(variable))
    var positions: Dictionary = {}
    var labels: Dictionary = {}
    var nodes: Array[Node] = _all_nodes(scene)
    for node in nodes:
        var path: String = str(scene.get_path_to(node))
        var key: String = str(node.get_meta("semwright_logical_key", path))
        if key.is_empty() or positions.has(key) or labels.has(key):
            _fail("runtime_logical_key_collision:" + key)
            continue
        if node is Node2D:
            positions[key] = _value(node.global_position, "runtime:position:" + key)
        elif node is Node3D:
            positions[key] = _value(node.global_position, "runtime:position:" + key)
        if node is Label:
            labels[key] = node.text.left(8192)
    var state: Variant = null
    var fault: Variant = null
    var ticks: Variant = null
    var events: Variant = null
    for field in ["sw_state", "sw_fault", "sw_ticks", "sw_events"]:
        if not root_properties.has(field):
            _fail("runtime_telemetry_missing:" + field)
    if root_properties.has("sw_state"): state = str(scene.get("sw_state"))
    if root_properties.has("sw_fault"): fault = str(scene.get("sw_fault"))
    if root_properties.has("sw_ticks"): ticks = int(scene.get("sw_ticks"))
    if root_properties.has("sw_events"): events = int(scene.get("sw_events"))
    if fault is String and fault.is_empty(): fault = null
    var capture_sha256: Variant = null
    if bool(_request.capture): capture_sha256 = _capture_digest()
    return {
        "requested_tick": requested_tick,
        "native_frame": str(Engine.get_physics_frames()),
        "scene_instance": str(scene.get_instance_id()),
        "scene_path": scene.scene_file_path,
        "state": state,
        "fault": fault,
        "ticks": ticks,
        "events": events,
        "variables": variables,
        "positions": positions,
        "labels": labels,
        "capture_sha256": capture_sha256,
    }
