@tool
extends RefCounted

static func body_inspect(ctx, args: Dictionary) -> Dictionary:
    var body = ctx._resolve_node(str(args.get("target", "")))
    if not (body is PhysicsBody3D):
        return ctx._error("not_found", "PhysicsBody3D not found")
    var data := {
        "target":str(args.get("target", "")),
        "class":body.get_class(),
        "collision_layer":body.collision_layer,
        "collision_mask":body.collision_mask,
    }
    if body is RigidBody3D:
        data.merge({
            "mass":body.mass,
            "gravity_scale":body.gravity_scale,
            "linear_damp":body.linear_damp,
            "angular_damp":body.angular_damp,
            "lock_rotation":body.lock_rotation,
            "freeze":body.freeze,
            "freeze_mode":body.freeze_mode,
            "continuous_cd":body.continuous_cd,
            "linear_velocity":_v3(body.linear_velocity),
            "angular_velocity":_v3(body.angular_velocity),
        })
    elif body is CharacterBody3D:
        data.merge({
            "motion_mode":body.motion_mode,
            "up_direction":_v3(body.up_direction),
            "velocity":_v3(body.velocity),
            "max_slides":body.max_slides,
            "floor_stop_on_slope":body.floor_stop_on_slope,
            "floor_max_angle":body.floor_max_angle,
            "floor_snap_length":body.floor_snap_length,
            "wall_min_slide_angle":body.wall_min_slide_angle,
        })
    elif body is StaticBody3D:
        data.merge({
            "constant_linear_velocity":_v3(body.constant_linear_velocity),
            "constant_angular_velocity":_v3(body.constant_angular_velocity),
        })
    return {"stamp":ctx._stamp(),"data":data}

static func body_configure(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var body = ctx._resolve_node(str(args.get("target", "")))
    if not (body is PhysicsBody3D): return ctx._error("not_found", "PhysicsBody3D not found")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [str(args.get("target", ""))], "dry-run")
    if args.has("collision_layer"): body.collision_layer = int(args["collision_layer"])
    if args.has("collision_mask"): body.collision_mask = int(args["collision_mask"])
    if body is RigidBody3D:
        for key in ["mass","gravity_scale","linear_damp","angular_damp"]:
            if args.has(key): body.set(key, float(args[key]))
        for key in ["lock_rotation","freeze","continuous_cd"]:
            if args.has(key): body.set(key, bool(args[key]))
        if args.has("freeze_mode"): body.freeze_mode = int(args["freeze_mode"])
        if args.has("linear_velocity"):
            var lv = _array_v3(args["linear_velocity"])
            if lv == null: return ctx._error("invalid_argument", "linear_velocity must be a 3-number array")
            body.linear_velocity = lv
        if args.has("angular_velocity"):
            var av = _array_v3(args["angular_velocity"])
            if av == null: return ctx._error("invalid_argument", "angular_velocity must be a 3-number array")
            body.angular_velocity = av
    elif body is CharacterBody3D:
        if args.has("motion_mode"): body.motion_mode = int(args["motion_mode"])
        if args.has("max_slides"): body.max_slides = int(args["max_slides"])
        for key in ["floor_max_angle","floor_snap_length","wall_min_slide_angle"]:
            if args.has(key): body.set(key, float(args[key]))
        if args.has("floor_stop_on_slope"): body.floor_stop_on_slope = bool(args["floor_stop_on_slope"])
        if args.has("up_direction"):
            var up = _array_v3(args["up_direction"])
            if up == null or up.is_zero_approx(): return ctx._error("invalid_argument", "up_direction must be a non-zero 3-number array")
            body.up_direction = up.normalized()
        if args.has("velocity"):
            var vel = _array_v3(args["velocity"])
            if vel == null: return ctx._error("invalid_argument", "velocity must be a 3-number array")
            body.velocity = vel
    elif body is StaticBody3D:
        if args.has("constant_linear_velocity"):
            var linear = _array_v3(args["constant_linear_velocity"])
            if linear == null: return ctx._error("invalid_argument", "constant_linear_velocity must be a 3-number array")
            body.constant_linear_velocity = linear
        if args.has("constant_angular_velocity"):
            var angular = _array_v3(args["constant_angular_velocity"])
            if angular == null: return ctx._error("invalid_argument", "constant_angular_velocity must be a 3-number array")
            body.constant_angular_velocity = angular
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("target", ""))], "Configure physics body")

static func area_inspect(ctx, args: Dictionary) -> Dictionary:
    var area = ctx._resolve_node(str(args.get("target", "")))
    if not (area is Area3D): return ctx._error("not_found", "Area3D not found")
    return {"stamp":ctx._stamp(),"data":{
        "target":str(args.get("target", "")),
        "monitoring":area.monitoring,
        "monitorable":area.monitorable,
        "priority":area.priority,
        "gravity_space_override":area.gravity_space_override,
        "gravity_point":area.gravity_point,
        "gravity":area.gravity,
        "linear_damp_space_override":area.linear_damp_space_override,
        "linear_damp":area.linear_damp,
        "angular_damp_space_override":area.angular_damp_space_override,
        "angular_damp":area.angular_damp,
        "audio_bus_override":area.audio_bus_override,
        "audio_bus_name":str(area.audio_bus_name),
        "collision_layer":area.collision_layer,
        "collision_mask":area.collision_mask,
    }}

static func area_configure(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var area = ctx._resolve_node(str(args.get("target", "")))
    if not (area is Area3D): return ctx._error("not_found", "Area3D not found")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [str(args.get("target", ""))], "dry-run")
    for key in ["monitoring","monitorable","gravity_point","audio_bus_override"]:
        if args.has(key): area.set(key, bool(args[key]))
    for key in ["priority","gravity","linear_damp","angular_damp"]:
        if args.has(key): area.set(key, float(args[key]))
    for key in ["gravity_space_override","linear_damp_space_override","angular_damp_space_override"]:
        if args.has(key): area.set(key, int(args[key]))
    if args.has("audio_bus_name"): area.audio_bus_name = StringName(str(args["audio_bus_name"]))
    if args.has("collision_layer"): area.collision_layer = int(args["collision_layer"])
    if args.has("collision_mask"): area.collision_mask = int(args["collision_mask"])
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("target", ""))], "Configure physics area")

static func joint_configure(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var joint = ctx._resolve_node(str(args.get("target", "")))
    if not (joint is Joint3D): return ctx._error("not_found", "Joint3D not found")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [str(args.get("target", ""))], "dry-run")
    if args.has("node_a"): joint.node_a = NodePath(str(args["node_a"]))
    if args.has("node_b"): joint.node_b = NodePath(str(args["node_b"]))
    if args.has("solver_priority"): joint.solver_priority = int(args["solver_priority"])
    if args.has("exclude_nodes_from_collision"): joint.exclude_nodes_from_collision = bool(args["exclude_nodes_from_collision"])
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("target", ""))], "Configure physics joint")

static func collision_shape_configure(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var node = ctx._resolve_node(str(args.get("target", "")))
    if not (node is CollisionShape3D): return ctx._error("not_found", "CollisionShape3D not found")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [str(args.get("target", ""))], "dry-run")
    if args.has("disabled"): node.disabled = bool(args["disabled"])
    if args.has("shape"):
        var path = str(args["shape"])
        if path.is_empty():
            node.shape = null
        elif ctx._safe_res(path) and ResourceLoader.exists(path):
            var shape = ResourceLoader.load(path, "Shape3D", ResourceLoader.CACHE_MODE_REUSE)
            if not (shape is Shape3D): return ctx._error("invalid_argument", "resource is not Shape3D")
            node.shape = shape
        else:
            return ctx._error("not_found", "Shape3D resource not found")
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("target", ""))], "Configure collision shape")

static func _array_v3(value):
    if not (value is Array) or value.size() != 3: return null
    return Vector3(float(value[0]),float(value[1]),float(value[2]))

static func _v3(value: Vector3) -> Array:
    return [value.x,value.y,value.z]
