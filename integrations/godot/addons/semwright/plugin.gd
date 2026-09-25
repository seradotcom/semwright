@tool
extends EditorPlugin

const BRIDGE_VERSION := 1
const MAX_NODES := 4000
const MAX_FILES := 4096
const MAX_EVENT_PAYLOAD_CHARS := 12000
const ProjectOps = preload("res://addons/semwright/ops/project_ops.gd")
const ResourceOps = preload("res://addons/semwright/ops/resource_ops.gd")
const ScriptOps = preload("res://addons/semwright/ops/script_ops.gd")
const SignalOps = preload("res://addons/semwright/ops/signal_ops.gd")
const AnimationOps = preload("res://addons/semwright/ops/animation_ops.gd")
const VisualOps = preload("res://addons/semwright/ops/visual_ops.gd")
const TileMapOps = preload("res://addons/semwright/ops/tilemap_ops.gd")
const NavigationOps = preload("res://addons/semwright/ops/navigation_ops.gd")
const PhysicsOps = preload("res://addons/semwright/ops/physics_ops.gd")
const AudioOps = preload("res://addons/semwright/ops/audio_ops.gd")
const ParticlesOps = preload("res://addons/semwright/ops/particles_ops.gd")
const RenderingOps = preload("res://addons/semwright/ops/rendering_ops.gd")
const UiThemeOps = preload("res://addons/semwright/ops/ui_theme_ops.gd")
const SkeletonOps = preload("res://addons/semwright/ops/skeleton_ops.gd")
const ProjectSemanticsOps = preload("res://addons/semwright/ops/project_semantics_ops.gd")
const AssetOps = preload("res://addons/semwright/ops/asset_ops.gd")
const ExportPresetOps = preload("res://addons/semwright/ops/export_preset_ops.gd")
const LocalizationOps = preload("res://addons/semwright/ops/localization_ops.gd")
const AnimationTreeOps = preload("res://addons/semwright/ops/animation_tree_ops.gd")
const MultiplayerOps = preload("res://addons/semwright/ops/multiplayer_ops.gd")
const EditorOps = preload("res://addons/semwright/ops/editor_ops.gd")

var _ws: WebSocketPeer
var _phase := "disconnected"
var _client_nonce := ""
var _generation := ""
var _session := ""
var _revision := 1
var _hello_sent := false
var _port := 0
var _project := ""
var _secret := ""

func _enter_tree() -> void:
    _bind_editor_events()
    _port = int(OS.get_environment("SEMWRIGHT_GODOT_PORT"))
    _project = OS.get_environment("SEMWRIGHT_GODOT_PROJECT")
    _secret = OS.get_environment("SEMWRIGHT_GODOT_SECRET")
    if _port <= 0 or _project.length() != 64 or _secret.length() != 64:
        # Offline loading is intentional for headless validation/export and for projects
        # where the owner has not paired Semwright. Do not warn in that case.
        return
    _connect_bridge()

func _exit_tree() -> void:
    if _ws != null:
        _ws.close(1000, "plugin disabled")
    _phase = "disconnected"

func _bind_editor_events() -> void:
    scene_changed.connect(_on_scene_changed)
    scene_closed.connect(_on_scene_closed)
    scene_saved.connect(_on_scene_saved)
    resource_saved.connect(_on_resource_saved)
    ProjectSettings.settings_changed.connect(_on_project_settings_changed)
    var filesystem := EditorInterface.get_resource_filesystem()
    filesystem.filesystem_changed.connect(_on_filesystem_changed)
    filesystem.resources_reimported.connect(_on_resources_reimported)

func _emit_event(kind: String, payload: Dictionary = {}) -> void:
    if _phase != "ready":
        return
    var encoded := JSON.stringify(payload)
    if kind.is_empty() or kind.length() > 64 or encoded.length() > MAX_EVENT_PAYLOAD_CHARS:
        return
    _send({
        "type": "event",
        "kind": kind,
        "revision": _revision,
        "payload": payload,
    })

func _on_scene_changed(scene_root: Node) -> void:
    _emit_event("scene_changed", {
        "path": "" if scene_root == null else str(scene_root.scene_file_path),
        "class": "" if scene_root == null else scene_root.get_class(),
    })

func _on_scene_closed(filepath: String) -> void:
    _emit_event("scene_closed", {"path": filepath})

func _on_scene_saved(filepath: String) -> void:
    _emit_event("scene_saved", {"path": filepath})

func _on_resource_saved(resource: Resource) -> void:
    _emit_event("resource_saved", {
        "path": resource.resource_path,
        "class": resource.get_class(),
    })

func _on_project_settings_changed() -> void:
    _emit_event("project_settings_changed")

func _on_filesystem_changed() -> void:
    _emit_event("filesystem_changed")

func _on_resources_reimported(resources: PackedStringArray) -> void:
    var bounded: Array[String] = []
    for path in resources:
        if bounded.size() >= 128:
            break
        bounded.append(str(path))
    _emit_event("resources_reimported", {"resources": bounded})

func _connect_bridge() -> void:
    _ws = WebSocketPeer.new()
    _ws.inbound_buffer_size = 524288
    _ws.outbound_buffer_size = 524288
    _ws.max_queued_packets = 64
    var err := _ws.connect_to_url("ws://127.0.0.1:%d" % _port)
    if err != OK:
        push_error("Semwright: WebSocket connect failed: %s" % error_string(err))
        return
    _phase = "connecting"
    _hello_sent = false
    set_process(true)

func _process(_delta: float) -> void:
    if _ws == null:
        return
    _ws.poll()
    var state := _ws.get_ready_state()
    if state == WebSocketPeer.STATE_OPEN:
        if not _hello_sent:
            _send_hello()
        while _ws.get_available_packet_count() > 0:
            var packet := _ws.get_packet()
            if not _ws.was_string_packet():
                _fail_connection("binary bridge frame rejected")
                return
            var parsed = JSON.parse_string(packet.get_string_from_utf8())
            if typeof(parsed) != TYPE_DICTIONARY:
                _fail_connection("invalid JSON bridge frame")
                return
            _handle_message(parsed)
    elif state == WebSocketPeer.STATE_CLOSED:
        if _phase != "disconnected":
            push_warning("Semwright: bridge disconnected.")
        _phase = "disconnected"
        set_process(false)

func _send_hello() -> void:
    _client_nonce = Crypto.new().generate_random_bytes(16).hex_encode()
    _send({
        "type": "hello",
        "project": _project,
        "nonce": _client_nonce,
        "plugin_version": "0.1.0",
        "engine_version": Engine.get_version_info().get("string", "unknown"),
    })
    _hello_sent = true
    _phase = "challenge"

func _handle_message(message: Dictionary) -> void:
    var kind := str(message.get("type", ""))
    if kind == "challenge" and _phase == "challenge":
        _handle_challenge(message)
    elif kind == "ready" and _phase == "authenticate":
        if int(message.get("bridge", -1)) != BRIDGE_VERSION:
            _fail_connection("bridge version mismatch")
            return
        _phase = "ready"
    elif kind == "request" and _phase == "ready":
        _handle_request(message)
    else:
        _fail_connection("unexpected bridge message")

func _handle_challenge(message: Dictionary) -> void:
    if int(message.get("bridge", -1)) != BRIDGE_VERSION:
        _fail_connection("bridge version mismatch")
        return
    var server_nonce := str(message.get("nonce", ""))
    _generation = str(message.get("generation", ""))
    _session = str(message.get("session", ""))
    if server_nonce.length() != 32 or _generation.length() != 32 or _session.length() != 32:
        _fail_connection("malformed challenge")
        return
    var server_transcript := _transcript("server", server_nonce)
    if not _ct_equal(str(message.get("proof", "")), _hmac(server_transcript)):
        _fail_connection("server authentication failed")
        return
    _send({"type":"authenticate","proof":_hmac(_transcript("client", server_nonce))})
    _phase = "authenticate"

func _transcript(role: String, server_nonce: String) -> String:
    return "SWG1\n%s\n%s\n%s\n%s\n%s\n%s" % [
        role, _project, _client_nonce, server_nonce, _generation, _session
    ]

func _hmac(text: String) -> String:
    var ctx := HMACContext.new()
    var err := ctx.start(HashingContext.HASH_SHA256, _secret.hex_decode())
    if err != OK:
        return ""
    if ctx.update(text.to_utf8_buffer()) != OK:
        return ""
    return ctx.finish().hex_encode()

func _ct_equal(a: String, b: String) -> bool:
    if a.length() != b.length():
        return false
    var diff := 0
    for i in a.length():
        diff |= a.unicode_at(i) ^ b.unicode_at(i)
    return diff == 0

func _send(value: Dictionary) -> void:
    if _ws != null and _ws.get_ready_state() == WebSocketPeer.STATE_OPEN:
        _ws.send_text(JSON.stringify(value))

func _fail_connection(reason: String) -> void:
    push_error("Semwright: %s" % reason)
    if _ws != null:
        _ws.close(1002, "protocol error")
    _phase = "disconnected"

func _handle_request(message: Dictionary) -> void:
    var id := str(message.get("id", ""))
    var op := str(message.get("op", ""))
    var args = message.get("args", {})
    if id.is_empty() or typeof(args) != TYPE_DICTIONARY:
        _send_error(id, "invalid_argument", "malformed request")
        return
    var result := _dispatch(op, args)
    if result.has("_error"):
        _send_error(id, str(result.get("_code", "backend_failed")), str(result["_error"]))
    else:
        _send({"type":"response","id":id,"ok":true,"value":result})

func _send_error(id: String, code: String, message: String) -> void:
    _send({"type":"response","id":id,"ok":false,"code":code,"message":message.left(512)})

func _dispatch(op: String, args: Dictionary) -> Dictionary:
    match op:
        "project.inspect": return _project_inspect()
        "project.files": return _project_files()
        "scene.inspect": return _scene_inspect()
        "scene.create": return _scene_create(args)
        "scene.open": return _scene_open(args)
        "scene.save": return _scene_save(args)
        "node.inspect": return _node_inspect(args)
        "node.create": return _node_create(args)
        "node.patch": return _node_patch(args)
        "node.remove": return _node_remove(args)
        "input.list": return _input_list()
        "input.set": return _input_set(args)
        "input.remove": return ProjectOps.input_remove(self, args)
        "scene.reload": return ProjectOps.scene_reload(self, args)
        "scene.instantiate": return ProjectOps.scene_instantiate(self, args)
        "node.rename": return ProjectOps.node_rename(self, args)
        "node.reparent": return ProjectOps.node_reparent(self, args)
        "group.set": return ProjectOps.group_set(self, args)
        "assets.status": return ProjectOps.assets_status(self, args)
        "assets.rescan": return ProjectOps.assets_rescan(self, args)
        "project.main_scene": return ProjectOps.main_scene(self, args)
        "resource.inspect": return ResourceOps.inspect(self, args)
        "resource.create": return ResourceOps.create(self, args)
        "resource.patch": return ResourceOps.patch(self, args)
        "resource.duplicate": return ResourceOps.duplicate_resource(self, args)
        "script.inspect": return ScriptOps.inspect(self, args)
        "script.write": return ScriptOps.write(self, args)
        "script.attach": return ScriptOps.attach(self, args)
        "script.detach": return ScriptOps.detach(self, args)
        "signal.list": return SignalOps.list(self, args)
        "signal.connect": return SignalOps.connect_signal(self, args)
        "signal.disconnect": return SignalOps.disconnect_signal(self, args)
        "animation.inspect": return AnimationOps.inspect(self, args)
        "animation.create": return AnimationOps.create(self, args)
        "animation.remove": return AnimationOps.remove(self, args)
        "animation.track.add": return AnimationOps.track_add(self, args)
        "animation.track.remove": return AnimationOps.track_remove(self, args)
        "animation.keyframe.set": return AnimationOps.keyframe_set(self, args)
        "animation.keyframe.remove": return AnimationOps.keyframe_remove(self, args)
        "animation_tree.configure": return AnimationOps.configure_tree(self, args)
        "physics.layers": return VisualOps.physics_layers(self, args)
        "ui.layout": return VisualOps.ui_layout(self, args)
        "shader.write": return VisualOps.shader_write(self, args)
        "shader.attach": return VisualOps.shader_attach(self, args)
        "tilemap.inspect": return TileMapOps.inspect(self, args)
        "tilemap.cell.set": return TileMapOps.cell_set(self, args)
        "tilemap.cell.erase": return TileMapOps.cell_erase(self, args)
        "tilemap.clear": return TileMapOps.clear(self, args)
        "tileset.inspect": return TileMapOps.tileset_inspect(self, args)
        "tileset.configure": return TileMapOps.tileset_configure(self, args)
        "tileset.atlas.create": return TileMapOps.atlas_create(self, args)
        "tileset.tile.create": return TileMapOps.tile_create(self, args)
        "navigation.region.inspect": return NavigationOps.region_inspect(self, args)
        "navigation.region.configure": return NavigationOps.region_configure(self, args)
        "navigation.region.bake": return NavigationOps.region_bake(self, args)
        "navigation.agent.inspect": return NavigationOps.agent_inspect(self, args)
        "navigation.agent.configure": return NavigationOps.agent_configure(self, args)
        "navigation.link.configure": return NavigationOps.link_configure(self, args)
        "physics.body.inspect": return PhysicsOps.body_inspect(self, args)
        "physics.body.configure": return PhysicsOps.body_configure(self, args)
        "physics.area.inspect": return PhysicsOps.area_inspect(self, args)
        "physics.area.configure": return PhysicsOps.area_configure(self, args)
        "physics.joint.configure": return PhysicsOps.joint_configure(self, args)
        "collision.shape.configure": return PhysicsOps.collision_shape_configure(self, args)
        "audio.player.inspect": return AudioOps.player_inspect(self, args)
        "audio.player.configure": return AudioOps.player_configure(self, args)
        "audio.bus.inspect": return AudioOps.bus_inspect(self, args)
        "audio.bus.create": return AudioOps.bus_create(self, args)
        "audio.bus.configure": return AudioOps.bus_configure(self, args)
        "audio.bus.remove": return AudioOps.bus_remove(self, args)
        "audio.effect.add": return AudioOps.effect_add(self, args)
        "audio.effect.remove": return AudioOps.effect_remove(self, args)
        "particles.inspect": return ParticlesOps.inspect(self, args)
        "particles.configure": return ParticlesOps.configure(self, args)
        "particles.restart": return ParticlesOps.restart(self, args)
        "particles.material.configure": return ParticlesOps.material_configure(self, args)
        "camera.inspect": return RenderingOps.camera_inspect(self, args)
        "camera.configure": return RenderingOps.camera_configure(self, args)
        "light.inspect": return RenderingOps.light_inspect(self, args)
        "light.configure": return RenderingOps.light_configure(self, args)
        "environment.inspect": return RenderingOps.environment_inspect(self, args)
        "environment.configure": return RenderingOps.environment_configure(self, args)
        "material.standard.inspect": return RenderingOps.material_inspect(self, args)
        "material.standard.configure": return RenderingOps.material_configure(self, args)
        "ui.control.inspect": return UiThemeOps.control_inspect(self, args)
        "ui.control.configure": return UiThemeOps.control_configure(self, args)
        "ui.text.configure": return UiThemeOps.text_configure(self, args)
        "theme.inspect": return UiThemeOps.theme_inspect(self, args)
        "theme.configure": return UiThemeOps.theme_configure(self, args)
        "theme.apply": return UiThemeOps.theme_apply(self, args)
        "skeleton.inspect": return SkeletonOps.inspect(self, args)
        "skeleton.bone.add": return SkeletonOps.bone_add(self, args)
        "skeleton.bone.configure": return SkeletonOps.bone_configure(self, args)
        "skeleton.attachment.configure": return SkeletonOps.attachment_configure(self, args)
        "project.window.inspect": return ProjectSemanticsOps.window_inspect(self, args)
        "project.window.configure": return ProjectSemanticsOps.window_configure(self, args)
        "project.rendering.inspect": return ProjectSemanticsOps.rendering_inspect(self, args)
        "project.rendering.configure": return ProjectSemanticsOps.rendering_configure(self, args)
        "project.physics.inspect": return ProjectSemanticsOps.physics_inspect(self, args)
        "project.physics.configure": return ProjectSemanticsOps.physics_configure(self, args)
        "project.layers.inspect": return ProjectSemanticsOps.layers_inspect(self, args)
        "project.layers.set": return ProjectSemanticsOps.layer_set(self, args)
        "autoload.list": return ProjectSemanticsOps.autoload_list(self, args)
        "autoload.add": return ProjectSemanticsOps.autoload_add(self, args)
        "autoload.remove": return ProjectSemanticsOps.autoload_remove(self, args)
        "asset.inspect": return AssetOps.inspect(self, args)
        "asset.dependencies": return AssetOps.dependencies(self, args)
        "asset.reimport": return AssetOps.reimport(self, args)
        "asset.import.inspect": return AssetOps.import_inspect(self, args)
        "asset.import.configure": return AssetOps.import_configure(self, args)
        "export.preset.list": return ExportPresetOps.list(self, args)
        "export.preset.inspect": return ExportPresetOps.inspect(self, args)
        "export.preset.configure": return ExportPresetOps.configure(self, args)
        "localization.inspect": return LocalizationOps.inspect(self, args)
        "localization.configure": return LocalizationOps.configure(self, args)
        "translation.inspect": return LocalizationOps.translation_inspect(self, args)
        "translation.create": return LocalizationOps.translation_create(self, args)
        "translation.message.set": return LocalizationOps.message_set(self, args)
        "translation.message.remove": return LocalizationOps.message_remove(self, args)
        "animation_tree.inspect": return AnimationTreeOps.inspect(self, args)
        "animation_tree.state.add": return AnimationTreeOps.state_add(self, args)
        "animation_tree.state.remove": return AnimationTreeOps.state_remove(self, args)
        "animation_tree.transition.add": return AnimationTreeOps.transition_add(self, args)
        "animation_tree.transition.configure": return AnimationTreeOps.transition_configure(self, args)
        "animation_tree.transition.remove": return AnimationTreeOps.transition_remove(self, args)
        "animation_tree.parameter.set": return AnimationTreeOps.parameter_set(self, args)
        "multiplayer.spawner.inspect": return MultiplayerOps.spawner_inspect(self, args)
        "multiplayer.spawner.configure": return MultiplayerOps.spawner_configure(self, args)
        "multiplayer.spawner.scene.add": return MultiplayerOps.spawner_scene_add(self, args)
        "multiplayer.spawner.scene.remove": return MultiplayerOps.spawner_scene_remove(self, args)
        "multiplayer.synchronizer.inspect": return MultiplayerOps.synchronizer_inspect(self, args)
        "multiplayer.synchronizer.configure": return MultiplayerOps.synchronizer_configure(self, args)
        "multiplayer.replication.property.add": return MultiplayerOps.replication_add(self, args)
        "multiplayer.replication.property.configure": return MultiplayerOps.replication_configure(self, args)
        "multiplayer.replication.property.remove": return MultiplayerOps.replication_remove(self, args)
        "editor.state": return EditorOps.state(self, args)
        "editor.selection.get": return EditorOps.selection_get(self, args)
        "editor.selection.set": return EditorOps.selection_set(self, args)
        "editor.run.start": return EditorOps.run_start(self, args)
        "editor.run.stop": return EditorOps.run_stop(self, args)
        _: return _error("unsupported", "unsupported Godot operation")

func _stamp() -> Dictionary:
    return {"revision":_revision,"fingerprint":_fingerprint()}

func _fingerprint() -> String:
    var root := EditorInterface.get_edited_scene_root()
    var rows: Array = []
    if root != null:
        _collect_nodes(root, root, rows)
    var input_rows: Array = []
    for action in _project_input_actions():
        var events: Array = []
        for event in action["events"]:
            var kind := str(event.get("type", ""))
            if kind == "joy_axis":
                events.append([
                    kind,
                    int(event.get("axis", 0)),
                    float(event.get("value", 0.0)),
                    int(event.get("device", -1)),
                ])
            else:
                events.append([
                    kind,
                    int(event.get("code", 0)),
                    int(event.get("device", -1)),
                ])
        input_rows.append([action["name"], action["deadzone"], events])
    var state := {
        "scene": "" if root == null else str(root.scene_file_path),
        "nodes": rows,
        "input": input_rows,
        "main_scene": str(ProjectSettings.get_setting("application/run/main_scene", "")),
    }
    var ctx := HashingContext.new()
    ctx.start(HashingContext.HASH_SHA256)
    ctx.update(JSON.stringify(state).to_utf8_buffer())
    return ctx.finish().hex_encode()

func _collect_nodes(root: Node, node: Node, rows: Array) -> void:
    if rows.size() >= MAX_NODES:
        return
    var stored := {}
    for p in node.get_property_list():
        if int(p.get("usage", 0)) & PROPERTY_USAGE_STORAGE == 0:
            continue
        var property_name := str(p.get("name", ""))
        if property_name in ["owner"]:
            continue
        var value = node.get(property_name)
        var normalized = _fingerprint_value(value)
        if normalized != null:
            stored[property_name] = normalized
    var groups: Array = node.get_groups()
    groups.sort()
    rows.append({
        "path": str(root.get_path_to(node)),
        "name": str(node.name),
        "class": node.get_class(),
        "groups": groups,
        "stored": stored,
    })
    for child in node.get_children():
        _collect_nodes(root, child, rows)

func _fingerprint_value(value):
    var type := typeof(value)
    if type in [TYPE_NIL,TYPE_BOOL,TYPE_INT,TYPE_FLOAT,TYPE_STRING]:
        return value
    if type in [TYPE_VECTOR2,TYPE_VECTOR3,TYPE_VECTOR4,TYPE_COLOR,TYPE_RECT2,TYPE_QUATERNION,TYPE_TRANSFORM2D,TYPE_TRANSFORM3D]:
        return str(value)
    if value is Resource:
        return {"resource": value.resource_path, "class": value.get_class()}
    if type in [TYPE_ARRAY,TYPE_DICTIONARY] and _json_safe(value):
        return value
    return null

func _check_expect(args: Dictionary) -> Dictionary:
    var expect = args.get("expect", {})
    if typeof(expect) != TYPE_DICTIONARY:
        return _error("invalid_argument", "expect must be an object")
    var current := _stamp()
    if int(expect.get("revision", -1)) != int(current["revision"]) or str(expect.get("fingerprint", "")) != str(current["fingerprint"]):
        return _error("conflict", "Godot editor state changed")
    return {}

func _mutation_result(applied: bool, affected: Array, undo: String) -> Dictionary:
    return {"applied":applied,"stamp":_stamp(),"affected":affected,"undo":undo}

func _project_inspect() -> Dictionary:
    var root := EditorInterface.get_edited_scene_root()
    return {
        "stamp":_stamp(),
        "data":{
            "name": str(ProjectSettings.get_setting("application/config/name", "")),
            "engine": str(Engine.get_version_info().get("string", "unknown")),
            "scene": "" if root == null else str(root.scene_file_path),
            "unsaved": not EditorInterface.get_unsaved_scenes().is_empty(),
        }
    }

func _project_files() -> Dictionary:
    var files: Array[String] = []
    _walk_files("res://", files)
    return {"stamp":_stamp(),"data":files}

func _walk_files(path: String, files: Array[String]) -> void:
    if files.size() >= MAX_FILES:
        return
    for file in DirAccess.get_files_at(path):
        if files.size() >= MAX_FILES:
            return
        files.append(path.path_join(file))
    for directory in DirAccess.get_directories_at(path):
        if not directory.begins_with("."):
            _walk_files(path.path_join(directory), files)

func _scene_inspect() -> Dictionary:
    var root := EditorInterface.get_edited_scene_root()
    var nodes: Array = []
    if root != null:
        _collect_nodes(root, root, nodes)
    return {"stamp":_stamp(),"data":{"scene":"" if root == null else str(root.scene_file_path),"nodes":nodes}}

func _scene_create(args: Dictionary) -> Dictionary:
    var conflict := _check_expect(args)
    if not conflict.is_empty(): return conflict
    var path := str(args.get("path", ""))
    if not _safe_res(path) or not path.ends_with(".tscn"):
        return _error("invalid_argument", "scene path must be a canonical res:// .tscn path")
    var klass := str(args.get("class", "Node"))
    if not ClassDB.class_exists(klass) or not ClassDB.is_parent_class(klass, "Node"):
        return _error("invalid_argument", "scene root class must derive from Node")
    if bool(args.get("dry_run", false)):
        return _mutation_result(false, [path], "dry-run")
    var root = ClassDB.instantiate(klass)
    if not (root is Node):
        return _error("backend_failed", "failed to instantiate scene root")
    root.name = str(args.get("name", "Root"))
    var packed := PackedScene.new()
    if packed.pack(root) != OK:
        root.free()
        return _error("backend_failed", "failed to pack scene")
    var err := ResourceSaver.save(packed, path)
    root.free()
    if err != OK:
        return _error("backend_failed", "failed to save scene")
    EditorInterface.open_scene_from_path(path)
    _revision += 1
    return _mutation_result(true, [path], "Create scene")

func _scene_open(args: Dictionary) -> Dictionary:
    var conflict := _check_expect(args)
    if not conflict.is_empty(): return conflict
    var path := str(args.get("path", ""))
    if not _safe_res(path):
        return _error("invalid_argument", "canonical res:// path required")
    if bool(args.get("dry_run", false)):
        return _mutation_result(false, [path], "dry-run")
    if not ResourceLoader.exists(path):
        return _error("not_found", "scene does not exist")
    EditorInterface.open_scene_from_path(path)
    _revision += 1
    return _mutation_result(true, [path], "Open scene")

func _scene_save(args: Dictionary) -> Dictionary:
    var conflict := _check_expect(args)
    if not conflict.is_empty(): return conflict
    var root := EditorInterface.get_edited_scene_root()
    if root == null:
        return _error("not_found", "no edited scene")
    if bool(args.get("dry_run", false)):
        return _mutation_result(false, [str(root.scene_file_path)], "dry-run")
    var err := EditorInterface.save_scene()
    if err != OK:
        return _error("backend_failed", "save_scene failed")
    _revision += 1
    return _mutation_result(true, [str(root.scene_file_path)], "Save scene")

func _node_inspect(args: Dictionary) -> Dictionary:
    var node := _resolve_node(str(args.get("path", "")))
    if node == null:
        return _error("not_found", "node not found")
    var props := {}
    for p in node.get_property_list():
        var usage := int(p.get("usage", 0))
        if usage & PROPERTY_USAGE_STORAGE != 0:
            var name := str(p.get("name", ""))
            if name in ["script", "owner"]:
                continue
            var value = node.get(name)
            if _json_safe(value):
                props[name] = value
    var root := EditorInterface.get_edited_scene_root()
    return {"stamp":_stamp(),"data":{"path":str(root.get_path_to(node)),"name":str(node.name),"class":node.get_class(),"properties":props}}

func _node_create(args: Dictionary) -> Dictionary:
    var conflict := _check_expect(args)
    if not conflict.is_empty(): return conflict
    var parent := _resolve_node(str(args.get("parent", ".")))
    if parent == null:
        return _error("not_found", "parent node not found")
    var klass := str(args.get("class", "Node"))
    if not ClassDB.class_exists(klass) or not ClassDB.is_parent_class(klass, "Node"):
        return _error("invalid_argument", "node class must derive from Node")
    var name := str(args.get("name", "Node"))
    if name.is_empty() or name.length() > 96:
        return _error("invalid_argument", "invalid node name")
    if bool(args.get("dry_run", false)):
        return _mutation_result(false, [str(args.get("parent", ".")) + "/" + name], "dry-run")
    var node = ClassDB.instantiate(klass)
    if not (node is Node):
        return _error("backend_failed", "failed to instantiate node")
    node.name = name
    for patch in args.get("properties", []):
        var result := _apply_property(node, patch)
        if not result.is_empty():
            node.free()
            return result
    parent.add_child(node)
    var root := EditorInterface.get_edited_scene_root()
    node.owner = root
    EditorInterface.mark_scene_as_unsaved()
    _revision += 1
    return _mutation_result(true, [str(root.get_path_to(node))], "Create node")

func _node_patch(args: Dictionary) -> Dictionary:
    var conflict := _check_expect(args)
    if not conflict.is_empty(): return conflict
    var node := _resolve_node(str(args.get("target", "")))
    if node == null:
        return _error("not_found", "target node not found")
    if bool(args.get("dry_run", false)):
        return _mutation_result(false, [str(args.get("target", ""))], "dry-run")
    for patch in args.get("properties", []):
        var result := _apply_property(node, patch)
        if not result.is_empty():
            return result
    EditorInterface.mark_scene_as_unsaved()
    _revision += 1
    return _mutation_result(true, [str(args.get("target", ""))], "Patch node")

func _node_remove(args: Dictionary) -> Dictionary:
    var conflict := _check_expect(args)
    if not conflict.is_empty(): return conflict
    var node := _resolve_node(str(args.get("target", "")))
    var root := EditorInterface.get_edited_scene_root()
    if node == null or node == root:
        return _error("invalid_argument", "cannot remove missing/root node")
    if bool(args.get("dry_run", false)):
        return _mutation_result(false, [str(args.get("target", ""))], "dry-run")
    var path := str(root.get_path_to(node))
    node.get_parent().remove_child(node)
    node.queue_free()
    EditorInterface.mark_scene_as_unsaved()
    _revision += 1
    return _mutation_result(true, [path], "Remove node")

func _apply_property(node: Node, patch: Dictionary) -> Dictionary:
    var name := str(patch.get("name", ""))
    if name in ["script", "owner", "scene_file_path"] or name.is_empty():
        return _error("permission_denied", "property is not writable through node.patch")
    var exists := false
    for p in node.get_property_list():
        if str(p.get("name", "")) == name and int(p.get("usage", 0)) & PROPERTY_USAGE_READ_ONLY == 0:
            exists = true
            break
    if not exists:
        return _error("invalid_argument", "unknown or read-only property")
    var encoded = patch.get("value")
    var value = _decode_value(encoded)
    if _is_resource_ref(encoded) and value == null:
        return _error("not_found", "referenced resource does not exist")
    node.set(name, value)
    return {}

func _decode_value(value):
    if typeof(value) == TYPE_DICTIONARY and value.has("$type"):
        var kind := str(value["$type"])
        var data = value.get("value", [])
        if kind == "Vector2" and data is Array and data.size() == 2:
            return Vector2(float(data[0]), float(data[1]))
        if kind == "Vector3" and data is Array and data.size() == 3:
            return Vector3(float(data[0]), float(data[1]), float(data[2]))
        if kind == "Color" and data is Array and data.size() in [3,4]:
            return Color(float(data[0]),float(data[1]),float(data[2]),1.0 if data.size()==3 else float(data[3]))
        if kind == "Resource":
            var path := str(value.get("path", ""))
            if _safe_res(path) and ResourceLoader.exists(path):
                return ResourceLoader.load(path, "", ResourceLoader.CACHE_MODE_REUSE)
            return null
    return value

func _is_resource_ref(value) -> bool:
    return typeof(value) == TYPE_DICTIONARY and str(value.get("$type", "")) == "Resource"

func _project_input_actions() -> Array:
    # Project InputMap bindings are persisted under input/* in ProjectSettings.
    # Preserve device semantics and analog axis direction instead of collapsing
    # every binding into keyboard/mouse-shaped data.
    var actions: Array = []
    for property in ProjectSettings.get_property_list():
        var setting_name := str(property.get("name", ""))
        if not setting_name.begins_with("input/"):
            continue
        var name := setting_name.trim_prefix("input/")
        if name.is_empty() or name.begins_with("ui_"):
            continue
        var setting = ProjectSettings.get_setting(setting_name, {})
        if typeof(setting) != TYPE_DICTIONARY:
            continue
        var events: Array = []
        for event in setting.get("events", []):
            if event is InputEventKey:
                events.append({"type":"key","code":event.physical_keycode,"device":event.device})
            elif event is InputEventMouseButton:
                events.append({"type":"mouse_button","code":event.button_index,"device":event.device})
            elif event is InputEventJoypadButton:
                events.append({"type":"joy_button","code":event.button_index,"device":event.device})
            elif event is InputEventJoypadMotion:
                events.append({"type":"joy_axis","axis":event.axis,"value":event.axis_value,"device":event.device})
        actions.append({
            "name": name,
            "deadzone": float(setting.get("deadzone", 0.5)),
            "events": events,
        })
    actions.sort_custom(func(a, b): return str(a["name"]) < str(b["name"]))
    return actions

func _input_list() -> Dictionary:
    return {"stamp":_stamp(),"data":_project_input_actions()}

func _input_set(args: Dictionary) -> Dictionary:
    var conflict := _check_expect(args)
    if not conflict.is_empty(): return conflict
    var name := str(args.get("name", ""))
    if name.is_empty() or name.begins_with("ui_"):
        return _error("invalid_argument", "invalid input action name")
    if bool(args.get("dry_run", false)):
        return _mutation_result(false, ["input/" + name], "dry-run")
    var deadzone := clampf(float(args.get("deadzone", 0.5)), 0.0, 1.0)
    var persisted_events: Array = []
    for spec in args.get("events", []):
        var event: InputEvent
        var kind := str(spec.get("type", ""))
        if kind == "key":
            var key := InputEventKey.new()
            key.physical_keycode = int(spec.get("code", 0))
            event = key
        elif kind == "mouse_button":
            var button := InputEventMouseButton.new()
            button.button_index = int(spec.get("code", 0))
            event = button
        elif kind == "joy_button":
            var joy_button := InputEventJoypadButton.new()
            joy_button.button_index = int(spec.get("code", 0))
            event = joy_button
        elif kind == "joy_axis":
            var joy_axis := InputEventJoypadMotion.new()
            joy_axis.axis = int(spec.get("axis", 0))
            joy_axis.axis_value = float(spec.get("value", 0.0))
            event = joy_axis
        if event != null:
            if spec.has("device"):
                event.device = int(spec["device"])
            persisted_events.append(event)
    ProjectSettings.set_setting("input/" + name, {
        "deadzone": deadzone,
        "events": persisted_events,
    })
    var save_error := ProjectSettings.save()
    if save_error != OK:
        return _error("backend_failed", "failed to persist input action")
    _revision += 1
    return _mutation_result(true, ["input/" + name], "Set input action")

func _resolve_node(path: String) -> Node:
    var root := EditorInterface.get_edited_scene_root()
    if root == null:
        return null
    if path in ["", "."]:
        return root
    if path.begins_with("/") or path.contains(".."):
        return null
    return root.get_node_or_null(NodePath(path))

func _safe_res(path: String) -> bool:
    if not path.begins_with("res://") or path.contains("..") or path.contains("\\"):
        return false
    return path.length() <= 240

func _json_safe(value) -> bool:
    return typeof(value) in [TYPE_NIL,TYPE_BOOL,TYPE_INT,TYPE_FLOAT,TYPE_STRING,TYPE_ARRAY,TYPE_DICTIONARY]

func _error(code: String, message: String) -> Dictionary:
    return {"_error":message,"_code":code}
