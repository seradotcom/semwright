@tool
extends RefCounted

static func inspect(ctx, args: Dictionary) -> Dictionary:
    var particles = ctx._resolve_node(str(args.get("target", "")))
    if not _is_particles(particles):
        return ctx._error("not_found", "particle emitter not found")
    var data := {
        "target":str(args.get("target", "")),
        "class":particles.get_class(),
        "amount":particles.amount,
        "lifetime":particles.lifetime,
        "emitting":particles.emitting,
        "one_shot":particles.one_shot,
        "preprocess":particles.preprocess,
        "randomness":particles.randomness,
        "speed_scale":particles.speed_scale,
        "local_coords":particles.local_coords,
    }
    for key in ["amount_ratio","explosiveness","fixed_fps","use_fixed_seed","seed","trail_enabled","trail_lifetime"]:
        if _has_property(particles,key): data[key] = particles.get(key)
    if _has_property(particles,"process_material"):
        var material = particles.get("process_material")
        data["process_material"] = "" if material == null else material.resource_path
    return {"stamp":ctx._stamp(),"data":data}

static func configure(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var particles = ctx._resolve_node(str(args.get("target", "")))
    if not _is_particles(particles): return ctx._error("not_found", "particle emitter not found")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [str(args.get("target", ""))], "dry-run")
    if args.has("amount"): particles.amount = int(args["amount"])
    for key in ["lifetime","preprocess","randomness","speed_scale","amount_ratio","explosiveness","trail_lifetime"]:
        if args.has(key):
            if not _has_property(particles,key): return ctx._error("invalid_argument", "%s unsupported by emitter" % key)
            particles.set(key,float(args[key]))
    for key in ["emitting","one_shot","local_coords","use_fixed_seed","trail_enabled"]:
        if args.has(key):
            if not _has_property(particles,key): return ctx._error("invalid_argument", "%s unsupported by emitter" % key)
            particles.set(key,bool(args[key]))
    for key in ["fixed_fps","seed"]:
        if args.has(key):
            if not _has_property(particles,key): return ctx._error("invalid_argument", "%s unsupported by emitter" % key)
            particles.set(key,int(args[key]))
    if args.has("process_material"):
        if not _has_property(particles,"process_material"): return ctx._error("invalid_argument", "emitter has no process material")
        var path = str(args["process_material"])
        if path.is_empty():
            particles.set("process_material",null)
        elif ctx._safe_res(path) and ResourceLoader.exists(path):
            var material = ResourceLoader.load(path, "Material", ResourceLoader.CACHE_MODE_REUSE)
            if not (material is Material): return ctx._error("invalid_argument", "process material is not Material")
            particles.set("process_material",material)
        else:
            return ctx._error("not_found", "process material not found")
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("target", ""))], "Configure particles")

static func restart(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var particles = ctx._resolve_node(str(args.get("target", "")))
    if not _is_particles(particles): return ctx._error("not_found", "particle emitter not found")
    if not particles.has_method("restart"): return ctx._error("unsupported", "particle emitter cannot restart")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [str(args.get("target", ""))], "dry-run")
    particles.restart()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("target", ""))], "Restart particles")

static func material_configure(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var path = str(args.get("path", ""))
    if not ctx._safe_res(path) or not ResourceLoader.exists(path):
        return ctx._error("not_found", "ParticleProcessMaterial not found")
    var material = ResourceLoader.load(path, "ParticleProcessMaterial", ResourceLoader.CACHE_MODE_REPLACE)
    if not (material is ParticleProcessMaterial):
        return ctx._error("invalid_argument", "resource is not ParticleProcessMaterial")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [path], "dry-run")
    if args.has("direction"):
        var direction = _array_v3(args["direction"])
        if direction == null or direction.is_zero_approx(): return ctx._error("invalid_argument", "direction must be a non-zero 3-number array")
        material.direction = direction.normalized()
    if args.has("gravity"):
        var gravity = _array_v3(args["gravity"])
        if gravity == null: return ctx._error("invalid_argument", "gravity must be a 3-number array")
        material.gravity = gravity
    if args.has("color"):
        var color = _array_color(args["color"])
        if color == null: return ctx._error("invalid_argument", "color must have 3 or 4 components")
        material.color = color
    if args.has("emission_box_extents"):
        var extents = _array_v3(args["emission_box_extents"])
        if extents == null: return ctx._error("invalid_argument", "emission_box_extents must be a 3-number array")
        material.emission_box_extents = extents
    if args.has("emission_shape"): material.emission_shape = int(args["emission_shape"])
    for key in ["spread","initial_velocity_min","initial_velocity_max","scale_min","scale_max","lifetime_randomness"]:
        if args.has(key): material.set(key,float(args[key]))
    if ResourceSaver.save(material,path) != OK:
        return ctx._error("backend_failed", "failed to save ParticleProcessMaterial")
    EditorInterface.get_resource_filesystem().update_file(path)
    ctx._revision += 1
    return ctx._mutation_result(true, [path], "Configure particle process material")

static func _is_particles(node) -> bool:
    return node is GPUParticles2D or node is GPUParticles3D or node is CPUParticles2D or node is CPUParticles3D

static func _has_property(object: Object, name: String) -> bool:
    for item in object.get_property_list():
        if str(item.get("name", "")) == name: return true
    return false

static func _array_v3(value):
    if not (value is Array) or value.size() != 3: return null
    return Vector3(float(value[0]),float(value[1]),float(value[2]))

static func _array_color(value):
    if not (value is Array) or value.size() not in [3,4]: return null
    return Color(float(value[0]),float(value[1]),float(value[2]),1.0 if value.size() == 3 else float(value[3]))
