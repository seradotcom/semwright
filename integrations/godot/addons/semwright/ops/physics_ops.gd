@tool
extends RefCounted

static func body_inspect(ctx, args: Dictionary) -> Dictionary:
    var body = ctx._resolve_node(str(args.get("target", "")))
    if body is PhysicsBody3D:
        return {"stamp":ctx._stamp(),"data":_body_data_3d(body, str(args.get("target", "")))}
    if body is PhysicsBody2D:
        return {"stamp":ctx._stamp(),"data":_body_data_2d(body, str(args.get("target", "")))}
    return ctx._error("not_found", "PhysicsBody2D or PhysicsBody3D not found")

static func body_configure(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var body = ctx._resolve_node(str(args.get("target", "")))
    if not (body is PhysicsBody2D) and not (body is PhysicsBody3D):
        return ctx._error("not_found", "PhysicsBody2D or PhysicsBody3D not found")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [str(args.get("target", ""))], "dry-run")
    if args.has("collision_layer"): body.collision_layer = int(args["collision_layer"])
    if args.has("collision_mask"): body.collision_mask = int(args["collision_mask"])
    var result := _configure_body_3d(ctx, body, args) if body is PhysicsBody3D else _configure_body_2d(ctx, body, args)
    if result.has("_error"): return result
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("target", ""))], "Configure physics body")

static func area_inspect(ctx, args: Dictionary) -> Dictionary:
    var area = ctx._resolve_node(str(args.get("target", "")))
    if not (area is Area2D) and not (area is Area3D):
        return ctx._error("not_found", "Area2D or Area3D not found")
    var data := {
        "target":str(args.get("target", "")),
        "dimension":"3d" if area is Area3D else "2d",
        "monitoring":area.monitoring,
        "monitorable":area.monitorable,
        "priority":area.priority,
        "gravity_space_override":area.gravity_space_override,
        "gravity_point":area.gravity_point,
        "gravity":area.gravity,
        "gravity_point_unit_distance":area.gravity_point_unit_distance,
        "linear_damp_space_override":area.linear_damp_space_override,
        "linear_damp":area.linear_damp,
        "angular_damp_space_override":area.angular_damp_space_override,
        "angular_damp":area.angular_damp,
        "audio_bus_override":area.audio_bus_override,
        "audio_bus_name":str(area.audio_bus_name),
        "collision_layer":area.collision_layer,
        "collision_mask":area.collision_mask,
    }
    if area is Area3D:
        data["gravity_direction"] = _v3(area.gravity_direction)
        data["gravity_point_center"] = _v3(area.gravity_point_center)
    else:
        data["gravity_direction"] = _v2(area.gravity_direction)
        data["gravity_point_center"] = _v2(area.gravity_point_center)
    return {"stamp":ctx._stamp(),"data":data}

static func area_configure(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var area = ctx._resolve_node(str(args.get("target", "")))
    if not (area is Area2D) and not (area is Area3D):
        return ctx._error("not_found", "Area2D or Area3D not found")
    if args.has("gravity_direction") and args.has("gravity_point_center"):
        return ctx._error("invalid_argument", "gravity_direction and gravity_point_center share one Godot gravity vector; configure only the active mode")
    var point_gravity := bool(args.get("gravity_point", area.gravity_point))
    if args.has("gravity_direction") and point_gravity:
        return ctx._error("invalid_argument", "gravity_direction requires gravity_point=false")
    if args.has("gravity_point_center") and not point_gravity:
        return ctx._error("invalid_argument", "gravity_point_center requires gravity_point=true")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [str(args.get("target", ""))], "dry-run")
    for key in ["monitoring","monitorable","gravity_point","audio_bus_override"]:
        if args.has(key): area.set(key, bool(args[key]))
    for key in ["priority","gravity","gravity_point_unit_distance","linear_damp","angular_damp"]:
        if args.has(key): area.set(key, float(args[key]))
    for key in ["gravity_space_override","linear_damp_space_override","angular_damp_space_override"]:
        if args.has(key): area.set(key, int(args[key]))
    if args.has("audio_bus_name"): area.audio_bus_name = StringName(str(args["audio_bus_name"]))
    if args.has("collision_layer"): area.collision_layer = int(args["collision_layer"])
    if args.has("collision_mask"): area.collision_mask = int(args["collision_mask"])
    if args.has("gravity_direction"):
        if area is Area3D:
            var direction3 = _array_v3(args["gravity_direction"])
            if direction3 == null: return ctx._error("invalid_argument", "3D gravity_direction must contain three numbers")
            area.gravity_direction = direction3
        else:
            var direction2 = _array_v2(args["gravity_direction"])
            if direction2 == null: return ctx._error("invalid_argument", "2D gravity_direction must contain two numbers")
            area.gravity_direction = direction2
    if args.has("gravity_point_center"):
        if area is Area3D:
            var center3 = _array_v3(args["gravity_point_center"])
            if center3 == null: return ctx._error("invalid_argument", "3D gravity_point_center must contain three numbers")
            area.gravity_point_center = center3
        else:
            var center2 = _array_v2(args["gravity_point_center"])
            if center2 == null: return ctx._error("invalid_argument", "2D gravity_point_center must contain two numbers")
            area.gravity_point_center = center2
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("target", ""))], "Configure physics area")

static func joint_configure(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var joint = ctx._resolve_node(str(args.get("target", "")))
    if not (joint is Joint2D) and not (joint is Joint3D):
        return ctx._error("not_found", "Joint2D or Joint3D not found")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [str(args.get("target", ""))], "dry-run")
    if args.has("node_a"): joint.node_a = NodePath(str(args["node_a"]))
    if args.has("node_b"): joint.node_b = NodePath(str(args["node_b"]))
    if joint is Joint3D:
        if args.has("solver_priority"): joint.solver_priority = int(args["solver_priority"])
        if args.has("bias"): return ctx._error("invalid_argument", "bias is only valid for Joint2D")
    else:
        if args.has("solver_priority"): return ctx._error("invalid_argument", "solver_priority is only valid for Joint3D")
        if args.has("bias"): joint.bias = float(args["bias"])
    if args.has("exclude_nodes_from_collision"):
        joint.set_exclude_nodes_from_collision(bool(args["exclude_nodes_from_collision"]))
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("target", ""))], "Configure physics joint")

static func collision_shape_configure(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var node = ctx._resolve_node(str(args.get("target", "")))
    if not (node is CollisionShape2D) and not (node is CollisionShape3D):
        return ctx._error("not_found", "CollisionShape2D or CollisionShape3D not found")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [str(args.get("target", ""))], "dry-run")
    if args.has("disabled"): node.disabled = bool(args["disabled"])
    if args.has("shape"):
        var path = str(args["shape"])
        if path.is_empty():
            node.shape = null
        elif not ctx._safe_res(path) or not ResourceLoader.exists(path):
            return ctx._error("not_found", "shape resource not found")
        elif node is CollisionShape3D:
            var shape3 = ResourceLoader.load(path, "Shape3D", ResourceLoader.CACHE_MODE_REUSE)
            if not (shape3 is Shape3D): return ctx._error("invalid_argument", "resource is not Shape3D")
            node.shape = shape3
        else:
            var shape2 = ResourceLoader.load(path, "Shape2D", ResourceLoader.CACHE_MODE_REUSE)
            if not (shape2 is Shape2D): return ctx._error("invalid_argument", "resource is not Shape2D")
            node.shape = shape2
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("target", ""))], "Configure collision shape")

static func _body_data_3d(body: PhysicsBody3D, target: String) -> Dictionary:
    var data := {"target":target,"dimension":"3d","class":body.get_class(),"collision_layer":body.collision_layer,"collision_mask":body.collision_mask}
    if body is RigidBody3D:
        data.merge({
            "mass":body.mass,"gravity_scale":body.gravity_scale,"linear_damp":body.linear_damp,
            "angular_damp":body.angular_damp,"lock_rotation":body.lock_rotation,"freeze":body.freeze,
            "freeze_mode":body.freeze_mode,"continuous_cd":body.continuous_cd,
            "linear_velocity":_v3(body.linear_velocity),"angular_velocity":_v3(body.angular_velocity),
        })
    elif body is CharacterBody3D:
        data.merge({
            "motion_mode":body.motion_mode,"up_direction":_v3(body.up_direction),"velocity":_v3(body.velocity),
            "max_slides":body.max_slides,"floor_stop_on_slope":body.floor_stop_on_slope,
            "floor_max_angle":body.floor_max_angle,"floor_snap_length":body.floor_snap_length,
            "wall_min_slide_angle":body.wall_min_slide_angle,
        })
    elif body is StaticBody3D:
        data.merge({"constant_linear_velocity":_v3(body.constant_linear_velocity),"constant_angular_velocity":_v3(body.constant_angular_velocity)})
    return data

static func _body_data_2d(body: PhysicsBody2D, target: String) -> Dictionary:
    var data := {"target":target,"dimension":"2d","class":body.get_class(),"collision_layer":body.collision_layer,"collision_mask":body.collision_mask}
    if body is RigidBody2D:
        data.merge({
            "mass":body.mass,"gravity_scale":body.gravity_scale,"linear_damp":body.linear_damp,
            "angular_damp":body.angular_damp,"lock_rotation":body.lock_rotation,"freeze":body.freeze,
            "freeze_mode":body.freeze_mode,"continuous_cd":body.continuous_cd,
            "linear_velocity":_v2(body.linear_velocity),"angular_velocity":body.angular_velocity,
        })
    elif body is CharacterBody2D:
        data.merge({
            "motion_mode":body.motion_mode,"up_direction":_v2(body.up_direction),"velocity":_v2(body.velocity),
            "max_slides":body.max_slides,"floor_stop_on_slope":body.floor_stop_on_slope,
            "floor_max_angle":body.floor_max_angle,"floor_snap_length":body.floor_snap_length,
            "wall_min_slide_angle":body.wall_min_slide_angle,
        })
    elif body is StaticBody2D:
        data.merge({"constant_linear_velocity":_v2(body.constant_linear_velocity),"constant_angular_velocity":body.constant_angular_velocity})
    return data

static func _configure_body_3d(ctx, body: PhysicsBody3D, args: Dictionary) -> Dictionary:
    if body is RigidBody3D:
        for key in ["mass","gravity_scale","linear_damp","angular_damp"]:
            if args.has(key): body.set(key, float(args[key]))
        for key in ["lock_rotation","freeze"]:
            if args.has(key): body.set(key, bool(args[key]))
        if args.has("continuous_cd"):
            if typeof(args["continuous_cd"]) != TYPE_BOOL: return ctx._error("invalid_argument", "RigidBody3D continuous_cd must be boolean")
            body.continuous_cd = bool(args["continuous_cd"])
        if args.has("freeze_mode"): body.freeze_mode = int(args["freeze_mode"])
        if args.has("linear_velocity"):
            var linear = _array_v3(args["linear_velocity"])
            if linear == null: return ctx._error("invalid_argument", "3D linear_velocity must contain three numbers")
            body.linear_velocity = linear
        if args.has("angular_velocity"):
            var angular = _array_v3(args["angular_velocity"])
            if angular == null: return ctx._error("invalid_argument", "3D angular_velocity must contain three numbers")
            body.angular_velocity = angular
    elif body is CharacterBody3D:
        var error = _configure_character(ctx, body, args, true)
        if not error.is_empty(): return error
    elif body is StaticBody3D:
        if args.has("constant_linear_velocity"):
            var linear3 = _array_v3(args["constant_linear_velocity"])
            if linear3 == null: return ctx._error("invalid_argument", "3D constant_linear_velocity must contain three numbers")
            body.constant_linear_velocity = linear3
        if args.has("constant_angular_velocity"):
            var angular3 = _array_v3(args["constant_angular_velocity"])
            if angular3 == null: return ctx._error("invalid_argument", "3D constant_angular_velocity must contain three numbers")
            body.constant_angular_velocity = angular3
    return {}

static func _configure_body_2d(ctx, body: PhysicsBody2D, args: Dictionary) -> Dictionary:
    if body is RigidBody2D:
        for key in ["mass","gravity_scale","linear_damp","angular_damp"]:
            if args.has(key): body.set(key, float(args[key]))
        for key in ["lock_rotation","freeze"]:
            if args.has(key): body.set(key, bool(args[key]))
        if args.has("continuous_cd"):
            if typeof(args["continuous_cd"]) != TYPE_INT: return ctx._error("invalid_argument", "RigidBody2D continuous_cd must be an enum integer")
            body.continuous_cd = int(args["continuous_cd"])
        if args.has("freeze_mode"): body.freeze_mode = int(args["freeze_mode"])
        if args.has("linear_velocity"):
            var linear = _array_v2(args["linear_velocity"])
            if linear == null: return ctx._error("invalid_argument", "2D linear_velocity must contain two numbers")
            body.linear_velocity = linear
        if args.has("angular_velocity"):
            if typeof(args["angular_velocity"]) not in [TYPE_INT,TYPE_FLOAT]: return ctx._error("invalid_argument", "2D angular_velocity must be numeric")
            body.angular_velocity = float(args["angular_velocity"])
    elif body is CharacterBody2D:
        var error = _configure_character(ctx, body, args, false)
        if not error.is_empty(): return error
    elif body is StaticBody2D:
        if args.has("constant_linear_velocity"):
            var linear2 = _array_v2(args["constant_linear_velocity"])
            if linear2 == null: return ctx._error("invalid_argument", "2D constant_linear_velocity must contain two numbers")
            body.constant_linear_velocity = linear2
        if args.has("constant_angular_velocity"):
            if typeof(args["constant_angular_velocity"]) not in [TYPE_INT,TYPE_FLOAT]: return ctx._error("invalid_argument", "2D constant_angular_velocity must be numeric")
            body.constant_angular_velocity = float(args["constant_angular_velocity"])
    return {}

static func _configure_character(ctx, body, args: Dictionary, is_3d: bool) -> Dictionary:
    if args.has("motion_mode"): body.motion_mode = int(args["motion_mode"])
    if args.has("max_slides"): body.max_slides = int(args["max_slides"])
    for key in ["floor_max_angle","floor_snap_length","wall_min_slide_angle"]:
        if args.has(key): body.set(key, float(args[key]))
    if args.has("floor_stop_on_slope"): body.floor_stop_on_slope = bool(args["floor_stop_on_slope"])
    if args.has("up_direction"):
        var up = _array_v3(args["up_direction"]) if is_3d else _array_v2(args["up_direction"])
        if up == null or up.is_zero_approx(): return ctx._error("invalid_argument", "up_direction must be a non-zero vector matching the body dimension")
        body.up_direction = up.normalized()
    if args.has("velocity"):
        var velocity = _array_v3(args["velocity"]) if is_3d else _array_v2(args["velocity"])
        if velocity == null: return ctx._error("invalid_argument", "velocity vector does not match body dimension")
        body.velocity = velocity
    return {}

static func _array_v2(value):
    if not (value is Array) or value.size() != 2: return null
    return Vector2(float(value[0]),float(value[1]))

static func _array_v3(value):
    if not (value is Array) or value.size() != 3: return null
    return Vector3(float(value[0]),float(value[1]),float(value[2]))

static func _v2(value: Vector2) -> Array:
    return [value.x,value.y]

static func _v3(value: Vector3) -> Array:
    return [value.x,value.y,value.z]
