@tool
extends RefCounted

const MAX_SPAWNABLE_SCENES := 256
const MAX_REPLICATION_PROPERTIES := 512

static func spawner_inspect(ctx, args: Dictionary) -> Dictionary:
    var node = ctx._resolve_node(str(args.get("target", "")))
    if not (node is MultiplayerSpawner):
        return ctx._error("not_found", "MultiplayerSpawner not found")
    var scenes: Array = []
    var count := mini(node.get_spawnable_scene_count(), MAX_SPAWNABLE_SCENES)
    for i in count:
        scenes.append(node.get_spawnable_scene(i))
    return {"stamp": ctx._stamp(), "data": {
        "target": str(args.get("target", "")),
        "spawn_path": str(node.spawn_path),
        "spawn_limit": node.spawn_limit,
        "spawnable_scenes": scenes,
        "truncated": node.get_spawnable_scene_count() > MAX_SPAWNABLE_SCENES,
    }}

static func spawner_configure(ctx, args: Dictionary) -> Dictionary:
    var node = ctx._resolve_node(str(args.get("target", "")))
    if not (node is MultiplayerSpawner):
        return ctx._error("not_found", "MultiplayerSpawner not found")
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty():
        return conflict
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [str(args.get("target", ""))], "dry-run")
    if args.has("spawn_path"):
        node.spawn_path = NodePath(str(args["spawn_path"]))
    if args.has("spawn_limit"):
        node.spawn_limit = int(args["spawn_limit"])
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("target", ""))], "Configure MultiplayerSpawner")
static func spawner_scene_add(ctx, args: Dictionary) -> Dictionary:
    var node = ctx._resolve_node(str(args.get("target", "")))
    if not (node is MultiplayerSpawner):
        return ctx._error("not_found", "MultiplayerSpawner not found")
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty():
        return conflict
    var scene := str(args.get("scene", ""))
    if not ctx._safe_res(scene) or not scene.ends_with(".tscn") or not ResourceLoader.exists(scene):
        return ctx._error("not_found", "spawnable scene does not exist")
    for i in node.get_spawnable_scene_count():
        if node.get_spawnable_scene(i) == scene:
            return ctx._error("conflict", "spawnable scene already registered")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [scene], "dry-run")
    node.add_spawnable_scene(scene)
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [scene], "Add spawnable scene")

static func spawner_scene_remove(ctx, args: Dictionary) -> Dictionary:
    var node = ctx._resolve_node(str(args.get("target", "")))
    if not (node is MultiplayerSpawner):
        return ctx._error("not_found", "MultiplayerSpawner not found")
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty():
        return conflict
    var scene := str(args.get("scene", ""))
    var keep: Array[String] = []
    var found := false
    for i in node.get_spawnable_scene_count():
        var current: String = node.get_spawnable_scene(i)
        if current == scene:
            found = true
        else:
            keep.append(current)
    if not found:
        return ctx._error("not_found", "spawnable scene is not registered")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [scene], "dry-run")
    node.clear_spawnable_scenes()
    for current in keep:
        node.add_spawnable_scene(current)
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [scene], "Remove spawnable scene")
static func synchronizer_inspect(ctx, args: Dictionary) -> Dictionary:
    var node = ctx._resolve_node(str(args.get("target", "")))
    if not (node is MultiplayerSynchronizer):
        return ctx._error("not_found", "MultiplayerSynchronizer not found")
    var properties: Array = []
    var config: SceneReplicationConfig = node.replication_config
    var truncated := false
    if config != null:
        for path in config.get_properties():
            if properties.size() >= MAX_REPLICATION_PROPERTIES:
                truncated = true
                break
            properties.append({
                "path": str(path),
                "spawn": config.property_get_spawn(path),
                "mode": config.property_get_replication_mode(path),
            })
    return {"stamp": ctx._stamp(), "data": {
        "target": str(args.get("target", "")),
        "root_path": str(node.root_path),
        "replication_interval": node.replication_interval,
        "delta_interval": node.delta_interval,
        "public_visibility": node.public_visibility,
        "visibility_update_mode": node.visibility_update_mode,
        "properties": properties,
        "truncated": truncated,
    }}
static func synchronizer_configure(ctx, args: Dictionary) -> Dictionary:
    var node = ctx._resolve_node(str(args.get("target", "")))
    if not (node is MultiplayerSynchronizer):
        return ctx._error("not_found", "MultiplayerSynchronizer not found")
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty():
        return conflict
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [str(args.get("target", ""))], "dry-run")
    if args.has("root_path"): node.root_path = NodePath(str(args["root_path"]))
    if args.has("replication_interval"): node.replication_interval = float(args["replication_interval"])
    if args.has("delta_interval"): node.delta_interval = float(args["delta_interval"])
    if args.has("public_visibility"): node.public_visibility = bool(args["public_visibility"])
    if args.has("visibility_update_mode"): node.visibility_update_mode = int(args["visibility_update_mode"])
    if node.replication_config == null:
        node.replication_config = SceneReplicationConfig.new()
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("target", ""))], "Configure MultiplayerSynchronizer")
static func replication_add(ctx, args: Dictionary) -> Dictionary:
    var resolved = _replication(ctx, args)
    if resolved is Dictionary:
        return resolved
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty():
        return conflict
    var config: SceneReplicationConfig = resolved
    var path := NodePath(str(args.get("path", "")))
    if path.is_empty():
        return ctx._error("invalid_argument", "replication property path is required")
    if config.has_property(path):
        return ctx._error("conflict", "replication property already exists")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [str(path)], "dry-run")
    config.add_property(path)
    config.property_set_spawn(path, bool(args.get("spawn", true)))
    config.property_set_replication_mode(path, int(args.get("mode", 1)))
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(path)], "Add replication property")

static func replication_configure(ctx, args: Dictionary) -> Dictionary:
    var resolved = _replication(ctx, args)
    if resolved is Dictionary:
        return resolved
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty():
        return conflict
    var config: SceneReplicationConfig = resolved
    var path := NodePath(str(args.get("path", "")))
    if not config.has_property(path):
        return ctx._error("not_found", "replication property not found")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [str(path)], "dry-run")
    if args.has("spawn"): config.property_set_spawn(path, bool(args["spawn"]))
    if args.has("mode"): config.property_set_replication_mode(path, int(args["mode"]))
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(path)], "Configure replication property")
static func replication_remove(ctx, args: Dictionary) -> Dictionary:
    var resolved = _replication(ctx, args)
    if resolved is Dictionary:
        return resolved
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty():
        return conflict
    var config: SceneReplicationConfig = resolved
    var path := NodePath(str(args.get("path", "")))
    if not config.has_property(path):
        return ctx._error("not_found", "replication property not found")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [str(path)], "dry-run")
    config.remove_property(path)
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(path)], "Remove replication property")

static func _replication(ctx, args: Dictionary):
    var node = ctx._resolve_node(str(args.get("target", "")))
    if not (node is MultiplayerSynchronizer):
        return ctx._error("not_found", "MultiplayerSynchronizer not found")
    if node.replication_config == null:
        node.replication_config = SceneReplicationConfig.new()
    return node.replication_config
