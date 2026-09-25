@tool
extends RefCounted

const MAX_POINTS := 2048

static func inspect(ctx, args: Dictionary) -> Dictionary:
    var path = ctx._resolve_node(str(args.get("target", "")))
    var curve = _curve(path)
    if curve == null:
        if not (path is Path2D) and not (path is Path3D):
            return ctx._error("not_found", "Path2D/Path3D not found")
        return {"stamp":ctx._stamp(),"data":{
            "target":str(args.get("target", "")),
            "dimension":2 if path is Path2D else 3,
            "has_curve":false,
            "bake_interval":0.0,
            "closed":false,
            "up_vector_enabled":false,
            "baked_length":0.0,
            "points":[],
            "truncated":false,
        }}
    var points: Array = []
    var count := mini(curve.point_count, MAX_POINTS)
    for i in count:
        var row := {
            "index":i,
            "position":_vector(curve.get_point_position(i)),
            "in":_vector(curve.get_point_in(i)),
            "out":_vector(curve.get_point_out(i)),
        }
        if curve is Curve3D:
            row["tilt"] = curve.get_point_tilt(i)
        points.append(row)
    return {"stamp":ctx._stamp(),"data":{
        "target":str(args.get("target", "")),
        "dimension":2 if curve is Curve2D else 3,
        "has_curve":true,
        "bake_interval":curve.bake_interval,
        "closed":false if curve is Curve2D else curve.closed,
        "up_vector_enabled":false if curve is Curve2D else curve.up_vector_enabled,
        "baked_length":curve.get_baked_length(),
        "points":points,
        "truncated":curve.point_count > MAX_POINTS,
    }}

static func configure(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var path = ctx._resolve_node(str(args.get("target", "")))
    if not (path is Path2D) and not (path is Path3D):
        return ctx._error("not_found", "Path2D/Path3D not found")
    var curve = _curve(path)
    if path is Path2D and (args.has("closed") or args.has("up_vector_enabled")):
        return ctx._error("invalid_argument", "closed/up_vector_enabled are Curve3D-only")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [str(args.get("target", ""))], "dry-run")
    if curve == null:
        curve = Curve2D.new() if path is Path2D else Curve3D.new()
        path.curve = curve
    if args.has("bake_interval"): curve.bake_interval = float(args["bake_interval"])
    if curve is Curve3D:
        if args.has("closed"): curve.closed = bool(args["closed"])
        if args.has("up_vector_enabled"): curve.up_vector_enabled = bool(args["up_vector_enabled"])
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("target", ""))], "Configure path curve")

static func point_add(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var path = ctx._resolve_node(str(args.get("target", "")))
    if not (path is Path2D) and not (path is Path3D):
        return ctx._error("not_found", "Path2D/Path3D not found")
    var curve = _curve(path)
    var dimension := 2 if path is Path2D else 3
    var pos = _vec(args.get("position"), dimension)
    var in_handle = _vec(args.get("in", []), dimension, true)
    var out_handle = _vec(args.get("out", []), dimension, true)
    if pos == null or in_handle == null or out_handle == null:
        return ctx._error("invalid_argument", "path point vectors do not match path dimension")
    var index := int(args.get("index", -1))
    var point_count := 0 if curve == null else curve.point_count
    if index < -1 or index > point_count:
        return ctx._error("invalid_argument", "path point insertion index is out of range")
    if dimension == 2 and args.has("tilt"):
        return ctx._error("invalid_argument", "tilt is Curve3D-only")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [str(args.get("target", ""))], "dry-run")
    if curve == null:
        curve = Curve2D.new() if path is Path2D else Curve3D.new()
        path.curve = curve
    curve.add_point(pos, in_handle, out_handle, index)
    var actual := curve.point_count - 1 if index < 0 else index
    if curve is Curve3D and args.has("tilt"): curve.set_point_tilt(actual, float(args["tilt"]))
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("target", "")), "point:%d" % actual], "Add path point")

static func point_configure(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var path = ctx._resolve_node(str(args.get("target", "")))
    var curve = _curve(path)
    if curve == null: return ctx._error("not_found", "path curve not found")
    var index := int(args.get("index", -1))
    if index < 0 or index >= curve.point_count:
        return ctx._error("not_found", "path point not found")
    var dimension := 2 if curve is Curve2D else 3
    if dimension == 2 and args.has("tilt"):
        return ctx._error("invalid_argument", "tilt is Curve3D-only")
    for key in ["position", "in", "out"]:
        if args.has(key) and _vec(args[key], dimension) == null:
            return ctx._error("invalid_argument", "path point vector has wrong dimension")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [str(args.get("target", "")), "point:%d" % index], "dry-run")
    if args.has("position"): curve.set_point_position(index, _vec(args["position"], dimension))
    if args.has("in"): curve.set_point_in(index, _vec(args["in"], dimension))
    if args.has("out"): curve.set_point_out(index, _vec(args["out"], dimension))
    if curve is Curve3D and args.has("tilt"): curve.set_point_tilt(index, float(args["tilt"]))
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("target", "")), "point:%d" % index], "Configure path point")

static func point_remove(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var path = ctx._resolve_node(str(args.get("target", "")))
    var curve = _curve(path)
    var index := int(args.get("index", -1))
    if curve == null or index < 0 or index >= curve.point_count:
        return ctx._error("not_found", "path point not found")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [str(args.get("target", "")), "point:%d" % index], "dry-run")
    curve.remove_point(index)
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("target", "")), "point:%d" % index], "Remove path point")

static func clear(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var path = ctx._resolve_node(str(args.get("target", "")))
    var curve = _curve(path)
    if curve == null: return ctx._error("not_found", "path curve not found")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [str(args.get("target", ""))], "dry-run")
    curve.clear_points()
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("target", ""))], "Clear path points")

static func follow_inspect(ctx, args: Dictionary) -> Dictionary:
    var follow = ctx._resolve_node(str(args.get("target", "")))
    if follow is PathFollow2D:
        return {"stamp":ctx._stamp(),"data":{
            "target":str(args.get("target", "")),"dimension":2,
            "progress":follow.progress,"progress_ratio":_progress_ratio(follow),
            "loop":follow.loop,"cubic_interp":follow.cubic_interp,
            "h_offset":follow.h_offset,"v_offset":follow.v_offset,
            "rotates":follow.rotates,"rotation_mode":0,
            "tilt_enabled":false,"use_model_front":false,
        }}
    if follow is PathFollow3D:
        return {"stamp":ctx._stamp(),"data":{
            "target":str(args.get("target", "")),"dimension":3,
            "progress":follow.progress,"progress_ratio":_progress_ratio(follow),
            "loop":follow.loop,"cubic_interp":follow.cubic_interp,
            "h_offset":follow.h_offset,"v_offset":follow.v_offset,
            "rotates":false,"rotation_mode":follow.rotation_mode,
            "tilt_enabled":follow.tilt_enabled,"use_model_front":follow.use_model_front,
        }}
    return ctx._error("not_found", "PathFollow2D/PathFollow3D not found")

static func follow_configure(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var follow = ctx._resolve_node(str(args.get("target", "")))
    if not (follow is PathFollow2D) and not (follow is PathFollow3D):
        return ctx._error("not_found", "PathFollow2D/PathFollow3D not found")
    if args.has("progress") and args.has("progress_ratio"):
        return ctx._error("invalid_argument", "set progress or progress_ratio, not both")
    if args.has("progress_ratio") and not _can_use_progress_ratio(follow):
        return ctx._error("invalid_argument", "progress_ratio requires a parent path with a non-zero curve")
    if follow is PathFollow2D and (args.has("rotation_mode") or args.has("tilt_enabled") or args.has("use_model_front")):
        return ctx._error("invalid_argument", "3D follower fields are invalid for PathFollow2D")
    if follow is PathFollow3D and args.has("rotates"):
        return ctx._error("invalid_argument", "rotates is PathFollow2D-only")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [str(args.get("target", ""))], "dry-run")
    if args.has("loop"): follow.loop = bool(args["loop"])
    if args.has("cubic_interp"): follow.cubic_interp = bool(args["cubic_interp"])
    if args.has("h_offset"): follow.h_offset = float(args["h_offset"])
    if args.has("v_offset"): follow.v_offset = float(args["v_offset"])
    if args.has("progress"): follow.progress = float(args["progress"])
    if args.has("progress_ratio"): follow.progress_ratio = float(args["progress_ratio"])
    if follow is PathFollow2D:
        if args.has("rotates"): follow.rotates = bool(args["rotates"])
    else:
        if args.has("rotation_mode"): follow.rotation_mode = int(args["rotation_mode"])
        if args.has("tilt_enabled"): follow.tilt_enabled = bool(args["tilt_enabled"])
        if args.has("use_model_front"): follow.use_model_front = bool(args["use_model_front"])
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("target", ""))], "Configure path follower")

static func _curve(path):
    if path is Path2D: return path.curve
    if path is Path3D: return path.curve
    return null

static func _can_use_progress_ratio(follow: Node) -> bool:
    var parent := follow.get_parent()
    var curve = _curve(parent)
    return curve != null and curve.get_baked_length() > 0.0

static func _progress_ratio(follow: Node) -> float:
    if not _can_use_progress_ratio(follow):
        return 0.0
    return follow.progress_ratio

static func _vec(value, dimension: int, allow_default: bool = false):
    if allow_default and value is Array and value.is_empty():
        return Vector2.ZERO if dimension == 2 else Vector3.ZERO
    if not (value is Array) or value.size() != dimension: return null
    if dimension == 2: return Vector2(float(value[0]), float(value[1]))
    return Vector3(float(value[0]), float(value[1]), float(value[2]))

static func _vector(value) -> Array:
    if value is Vector2: return [value.x, value.y]
    return [value.x, value.y, value.z]

