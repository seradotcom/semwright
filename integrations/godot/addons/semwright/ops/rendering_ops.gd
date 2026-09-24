@tool
extends RefCounted

static func camera_inspect(ctx, args: Dictionary) -> Dictionary:
    var camera = ctx._resolve_node(str(args.get("target", "")))
    if camera is Camera3D:
        var environment := ""
        if camera.environment != null: environment = camera.environment.resource_path
        return {"stamp":ctx._stamp(),"data":{
            "target":str(args.get("target", "")),"class":camera.get_class(),
            "projection":camera.projection,"fov":camera.fov,"size":camera.size,
            "near":camera.near,"far":camera.far,"keep_aspect":camera.keep_aspect,
            "current":camera.current,"cull_mask":camera.cull_mask,"environment":environment,
        }}
    if camera is Camera2D:
        return {"stamp":ctx._stamp(),"data":{
            "target":str(args.get("target", "")),"class":camera.get_class(),
            "enabled":camera.enabled,"zoom":_v2(camera.zoom),"offset":_v2(camera.offset),
            "ignore_rotation":camera.ignore_rotation,
            "position_smoothing_enabled":camera.position_smoothing_enabled,
            "position_smoothing_speed":camera.position_smoothing_speed,
            "rotation_smoothing_enabled":camera.rotation_smoothing_enabled,
            "rotation_smoothing_speed":camera.rotation_smoothing_speed,
            "limit_enabled":camera.limit_enabled,
            "limits":[camera.limit_left,camera.limit_top,camera.limit_right,camera.limit_bottom],
        }}
    return ctx._error("not_found", "Camera2D or Camera3D not found")

static func camera_configure(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var camera = ctx._resolve_node(str(args.get("target", "")))
    if not (camera is Camera2D) and not (camera is Camera3D):
        return ctx._error("not_found", "Camera2D or Camera3D not found")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [str(args.get("target", ""))], "dry-run")
    if camera is Camera3D:
        for key in ["fov","size","near","far"]:
            if args.has(key): camera.set(key,float(args[key]))
        for key in ["projection","keep_aspect","cull_mask"]:
            if args.has(key): camera.set(key,int(args[key]))
        if args.has("current"): camera.current = bool(args["current"])
        if args.has("environment"):
            var path = str(args["environment"])
            if path.is_empty():
                camera.environment = null
            elif ctx._safe_res(path) and ResourceLoader.exists(path):
                var env = ResourceLoader.load(path, "Environment", ResourceLoader.CACHE_MODE_REUSE)
                if not (env is Environment): return ctx._error("invalid_argument", "resource is not Environment")
                camera.environment = env
            else:
                return ctx._error("not_found", "Environment resource not found")
    else:
        for key in ["enabled","ignore_rotation","position_smoothing_enabled","rotation_smoothing_enabled","limit_enabled"]:
            if args.has(key): camera.set(key,bool(args[key]))
        for key in ["position_smoothing_speed","rotation_smoothing_speed"]:
            if args.has(key): camera.set(key,float(args[key]))
        if args.has("zoom"):
            var zoom = _array_v2(args["zoom"])
            if zoom == null or zoom.x <= 0.0 or zoom.y <= 0.0: return ctx._error("invalid_argument", "zoom must be a positive 2-number array")
            camera.zoom = zoom
        if args.has("offset"):
            var offset = _array_v2(args["offset"])
            if offset == null: return ctx._error("invalid_argument", "offset must be a 2-number array")
            camera.offset = offset
        if args.has("limits"):
            var limits = args["limits"]
            if not (limits is Array) or limits.size() != 4: return ctx._error("invalid_argument", "limits must have left,top,right,bottom")
            camera.limit_left = int(limits[0]); camera.limit_top = int(limits[1])
            camera.limit_right = int(limits[2]); camera.limit_bottom = int(limits[3])
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("target", ""))], "Configure camera")

static func light_inspect(ctx, args: Dictionary) -> Dictionary:
    var light = ctx._resolve_node(str(args.get("target", "")))
    if light is Light3D:
        return {"stamp":ctx._stamp(),"data":{
            "target":str(args.get("target", "")),"class":light.get_class(),
            "color":_color(light.light_color),"energy":light.light_energy,
            "indirect_energy":light.light_indirect_energy,"specular":light.light_specular,
            "shadow_enabled":light.shadow_enabled,"cull_mask":light.light_cull_mask,
            "volumetric_fog_energy":light.light_volumetric_fog_energy,
        }}
    if light is Light2D:
        return {"stamp":ctx._stamp(),"data":{
            "target":str(args.get("target", "")),"class":light.get_class(),
            "color":_color(light.color),"energy":light.energy,"enabled":light.enabled,
            "shadow_enabled":light.shadow_enabled,"blend_mode":light.blend_mode,
            "range_item_cull_mask":light.range_item_cull_mask,
            "shadow_item_cull_mask":light.shadow_item_cull_mask,
        }}
    return ctx._error("not_found", "Light2D or Light3D not found")

static func light_configure(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var light = ctx._resolve_node(str(args.get("target", "")))
    if not (light is Light2D) and not (light is Light3D):
        return ctx._error("not_found", "Light2D or Light3D not found")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [str(args.get("target", ""))], "dry-run")
    if args.has("color"):
        var color = _array_color(args["color"])
        if color == null: return ctx._error("invalid_argument", "color must have 3 or 4 components")
        if light is Light3D: light.light_color = color
        else: light.color = color
    if light is Light3D:
        for key in ["light_energy","light_indirect_energy","light_specular","light_volumetric_fog_energy"]:
            var short = key.trim_prefix("light_")
            if args.has(short): light.set(key,float(args[short]))
        if args.has("shadow_enabled"): light.shadow_enabled = bool(args["shadow_enabled"])
        if args.has("cull_mask"): light.light_cull_mask = int(args["cull_mask"])
    else:
        if args.has("energy"): light.energy = float(args["energy"])
        if args.has("enabled"): light.enabled = bool(args["enabled"])
        if args.has("shadow_enabled"): light.shadow_enabled = bool(args["shadow_enabled"])
        if args.has("blend_mode"): light.blend_mode = int(args["blend_mode"])
        if args.has("cull_mask"): light.range_item_cull_mask = int(args["cull_mask"])
        if args.has("shadow_cull_mask"): light.shadow_item_cull_mask = int(args["shadow_cull_mask"])
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("target", ""))], "Configure light")

static func environment_inspect(ctx, args: Dictionary) -> Dictionary:
    var env = _environment(ctx,str(args.get("path", "")))
    if env == null: return ctx._error("not_found", "Environment resource not found")
    return {"stamp":ctx._stamp(),"data":{
        "path":env.resource_path,
        "background_mode":env.background_mode,
        "background_color":_color(env.background_color),
        "background_energy_multiplier":env.background_energy_multiplier,
        "ambient_light_source":env.ambient_light_source,
        "ambient_light_color":_color(env.ambient_light_color),
        "ambient_light_energy":env.ambient_light_energy,
        "fog_enabled":env.fog_enabled,"fog_density":env.fog_density,
        "fog_light_color":_color(env.fog_light_color),"fog_light_energy":env.fog_light_energy,
        "glow_enabled":env.glow_enabled,"glow_intensity":env.glow_intensity,
        "tonemap_mode":env.tonemap_mode,
    }}

static func environment_configure(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var env = _environment(ctx,str(args.get("path", "")))
    if env == null: return ctx._error("not_found", "Environment resource not found")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [env.resource_path], "dry-run")
    for key in ["background_mode","ambient_light_source","tonemap_mode"]:
        if args.has(key): env.set(key,int(args[key]))
    for key in ["background_energy_multiplier","ambient_light_energy","fog_density","fog_light_energy","glow_intensity"]:
        if args.has(key): env.set(key,float(args[key]))
    for key in ["fog_enabled","glow_enabled"]:
        if args.has(key): env.set(key,bool(args[key]))
    for key in ["background_color","ambient_light_color","fog_light_color"]:
        if args.has(key):
            var color = _array_color(args[key])
            if color == null: return ctx._error("invalid_argument", "%s must be a color array" % key)
            env.set(key,color)
    if ResourceSaver.save(env,env.resource_path) != OK:
        return ctx._error("backend_failed", "failed to save Environment")
    EditorInterface.get_resource_filesystem().update_file(env.resource_path)
    ctx._revision += 1
    return ctx._mutation_result(true, [env.resource_path], "Configure environment")

static func material_inspect(ctx, args: Dictionary) -> Dictionary:
    var material = _standard_material(ctx,str(args.get("path", "")))
    if material == null: return ctx._error("not_found", "StandardMaterial3D not found")
    return {"stamp":ctx._stamp(),"data":{
        "path":material.resource_path,"albedo_color":_color(material.albedo_color),
        "metallic":material.metallic,"roughness":material.roughness,
        "emission_enabled":material.emission_enabled,"emission":_color(material.emission),
        "transparency":material.transparency,"shading_mode":material.shading_mode,
    }}

static func material_configure(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var material = _standard_material(ctx,str(args.get("path", "")))
    if material == null: return ctx._error("not_found", "StandardMaterial3D not found")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [material.resource_path], "dry-run")
    for key in ["metallic","roughness"]:
        if args.has(key): material.set(key,float(args[key]))
    for key in ["emission_enabled"]:
        if args.has(key): material.set(key,bool(args[key]))
    for key in ["transparency","shading_mode"]:
        if args.has(key): material.set(key,int(args[key]))
    for key in ["albedo_color","emission"]:
        if args.has(key):
            var color = _array_color(args[key])
            if color == null: return ctx._error("invalid_argument", "%s must be a color array" % key)
            material.set(key,color)
    if ResourceSaver.save(material,material.resource_path) != OK:
        return ctx._error("backend_failed", "failed to save StandardMaterial3D")
    EditorInterface.get_resource_filesystem().update_file(material.resource_path)
    ctx._revision += 1
    return ctx._mutation_result(true, [material.resource_path], "Configure standard material")

static func _environment(ctx, path: String) -> Environment:
    if not ctx._safe_res(path) or not ResourceLoader.exists(path): return null
    return ResourceLoader.load(path,"Environment",ResourceLoader.CACHE_MODE_REPLACE) as Environment

static func _standard_material(ctx, path: String) -> StandardMaterial3D:
    if not ctx._safe_res(path) or not ResourceLoader.exists(path): return null
    return ResourceLoader.load(path,"StandardMaterial3D",ResourceLoader.CACHE_MODE_REPLACE) as StandardMaterial3D

static func _array_v2(value):
    if not (value is Array) or value.size() != 2: return null
    return Vector2(float(value[0]),float(value[1]))

static func _v2(value: Vector2) -> Array:
    return [value.x,value.y]

static func _array_color(value):
    if not (value is Array) or value.size() not in [3,4]: return null
    return Color(float(value[0]),float(value[1]),float(value[2]),1.0 if value.size()==3 else float(value[3]))

static func _color(value: Color) -> Array:
    return [value.r,value.g,value.b,value.a]
