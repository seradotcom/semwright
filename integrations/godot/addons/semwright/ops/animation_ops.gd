@tool
extends RefCounted

static func inspect(ctx, args: Dictionary) -> Dictionary:
    var player = _player(ctx, str(args.get("player", "")))
    if player == null: return ctx._error("not_found", "AnimationPlayer not found")
    var libraries: Array = []
    for library_name in player.get_animation_library_list():
        var library = player.get_animation_library(library_name)
        var animations: Array = []
        for animation_name in library.get_animation_list():
            var animation = library.get_animation(animation_name)
            var tracks: Array = []
            for i in animation.get_track_count():
                var keys: Array = []
                for k in animation.track_get_key_count(i):
                    var value = animation.track_get_key_value(i, k)
                    keys.append({
                        "time": animation.track_get_key_time(i, k),
                        "transition": animation.track_get_key_transition(i, k),
                        "value": ctx._encode_value(value),
                    })
                tracks.append({
                    "index":i,
                    "type":animation.track_get_type(i),
                    "path":str(animation.track_get_path(i)),
                    "enabled":animation.track_is_enabled(i),
                    "keys":keys,
                })
            animations.append({"name":str(animation_name),"length":animation.length,"loop_mode":animation.loop_mode,"tracks":tracks})
        libraries.append({"name":str(library_name),"animations":animations})
    return {"stamp":ctx._stamp(),"data":{"libraries":libraries}}

static func create(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var player = _player(ctx, str(args.get("player", "")))
    if player == null: return ctx._error("not_found", "AnimationPlayer not found")
    var library_name = str(args.get("library", ""))
    var animation_name = str(args.get("animation", ""))
    if animation_name.is_empty(): return ctx._error("invalid_argument", "animation name is required")
    if bool(args.get("dry_run", false)): return ctx._mutation_result(false, [animation_name], "dry-run")
    var library: AnimationLibrary
    if player.has_animation_library(library_name):
        library = player.get_animation_library(library_name)
    else:
        library = AnimationLibrary.new()
        if player.add_animation_library(library_name, library) != OK:
            return ctx._error("backend_failed", "failed to add animation library")
    if library.has_animation(animation_name): return ctx._error("conflict", "animation already exists")
    var animation = Animation.new()
    animation.length = float(args.get("length", 1.0))
    animation.loop_mode = int(args.get("loop_mode", Animation.LOOP_NONE))
    if library.add_animation(animation_name, animation) != OK:
        return ctx._error("backend_failed", "failed to add animation")
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [animation_name], "Create animation")

static func remove(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var player = _player(ctx, str(args.get("player", "")))
    if player == null: return ctx._error("not_found", "AnimationPlayer not found")
    var library_name = str(args.get("library", ""))
    var animation_name = str(args.get("animation", ""))
    if not player.has_animation_library(library_name): return ctx._error("not_found", "animation library not found")
    var library = player.get_animation_library(library_name)
    if not library.has_animation(animation_name): return ctx._error("not_found", "animation not found")
    if bool(args.get("dry_run", false)): return ctx._mutation_result(false, [animation_name], "dry-run")
    library.remove_animation(animation_name)
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [animation_name], "Remove animation")

static func track_add(ctx, args: Dictionary) -> Dictionary:
    var resolved = _animation(ctx, args)
    if resolved.has("_error"): return resolved
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var animation: Animation = resolved["animation"]
    var track_type = _track_type(str(args.get("type", "value")))
    if track_type < 0: return ctx._error("invalid_argument", "unsupported animation track type")
    if bool(args.get("dry_run", false)): return ctx._mutation_result(false, [str(args.get("path", ""))], "dry-run")
    var index = animation.add_track(track_type)
    animation.track_set_path(index, NodePath(str(args.get("path", ""))))
    animation.track_set_enabled(index, bool(args.get("enabled", true)))
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, ["track:%d" % index], "Add animation track")

static func track_remove(ctx, args: Dictionary) -> Dictionary:
    var resolved = _animation(ctx, args)
    if resolved.has("_error"): return resolved
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var animation: Animation = resolved["animation"]
    var track = int(args.get("track", -1))
    if track < 0 or track >= animation.get_track_count(): return ctx._error("not_found", "animation track not found")
    if bool(args.get("dry_run", false)): return ctx._mutation_result(false, ["track:%d" % track], "dry-run")
    animation.remove_track(track)
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, ["track:%d" % track], "Remove animation track")

static func keyframe_set(ctx, args: Dictionary) -> Dictionary:
    var resolved = _animation(ctx, args)
    if resolved.has("_error"): return resolved
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var animation: Animation = resolved["animation"]
    var track = int(args.get("track", -1))
    if track < 0 or track >= animation.get_track_count(): return ctx._error("not_found", "animation track not found")
    var time = float(args.get("time", -1.0))
    if time < 0.0 or not is_finite(time): return ctx._error("invalid_argument", "keyframe time must be finite and non-negative")
    if bool(args.get("dry_run", false)): return ctx._mutation_result(false, ["track:%d" % track], "dry-run")
    var value = ctx._decode_value(args.get("value"))
    animation.track_insert_key(track, time, value, float(args.get("transition", 1.0)))
    if time > animation.length: animation.length = time
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, ["track:%d@%s" % [track,time]], "Set animation keyframe")

static func keyframe_remove(ctx, args: Dictionary) -> Dictionary:
    var resolved = _animation(ctx, args)
    if resolved.has("_error"): return resolved
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var animation: Animation = resolved["animation"]
    var track = int(args.get("track", -1))
    var time = float(args.get("time", -1.0))
    if track < 0 or track >= animation.get_track_count() or time < 0.0:
        return ctx._error("invalid_argument", "invalid keyframe target")
    if bool(args.get("dry_run", false)): return ctx._mutation_result(false, ["track:%d" % track], "dry-run")
    animation.track_remove_key_at_time(track, time)
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, ["track:%d@%s" % [track,time]], "Remove animation keyframe")

static func configure_tree(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var tree = ctx._resolve_node(str(args.get("tree", "")))
    if not (tree is AnimationTree): return ctx._error("not_found", "AnimationTree not found")
    if bool(args.get("dry_run", false)): return ctx._mutation_result(false, [str(args.get("tree", ""))], "dry-run")
    if args.has("active"): tree.active = bool(args["active"])
    if args.has("root_motion_track"): tree.root_motion_track = NodePath(str(args["root_motion_track"]))
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("tree", ""))], "Configure AnimationTree")

static func _player(ctx, path: String) -> AnimationPlayer:
    var node = ctx._resolve_node(path)
    return node as AnimationPlayer

static func _animation(ctx, args: Dictionary) -> Dictionary:
    var player = _player(ctx, str(args.get("player", "")))
    if player == null: return ctx._error("not_found", "AnimationPlayer not found")
    var library_name = str(args.get("library", ""))
    var animation_name = str(args.get("animation", ""))
    if not player.has_animation_library(library_name): return ctx._error("not_found", "animation library not found")
    var library = player.get_animation_library(library_name)
    if not library.has_animation(animation_name): return ctx._error("not_found", "animation not found")
    return {"animation":library.get_animation(animation_name)}

static func _track_type(kind: String) -> int:
    match kind:
        "value": return Animation.TYPE_VALUE
        "position_3d": return Animation.TYPE_POSITION_3D
        "rotation_3d": return Animation.TYPE_ROTATION_3D
        "scale_3d": return Animation.TYPE_SCALE_3D
        "blend_shape": return Animation.TYPE_BLEND_SHAPE
        "bezier": return Animation.TYPE_BEZIER
        "method": return Animation.TYPE_METHOD
        "audio": return Animation.TYPE_AUDIO
        "animation": return Animation.TYPE_ANIMATION
        _: return -1
