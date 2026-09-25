@tool
extends RefCounted

const WINDOW_SETTINGS := {
    "viewport_width": "display/window/size/viewport_width",
    "viewport_height": "display/window/size/viewport_height",
    "window_width_override": "display/window/size/window_width_override",
    "window_height_override": "display/window/size/window_height_override",
    "mode": "display/window/size/mode",
    "resizable": "display/window/size/resizable",
    "borderless": "display/window/size/borderless",
    "always_on_top": "display/window/size/always_on_top",
    "stretch_mode": "display/window/stretch/mode",
    "stretch_aspect": "display/window/stretch/aspect",
}

const RENDERING_SETTINGS := {
    "rendering_method": "rendering/renderer/rendering_method",
    "rendering_method_mobile": "rendering/renderer/rendering_method.mobile",
    "msaa_2d": "rendering/anti_aliasing/quality/msaa_2d",
    "msaa_3d": "rendering/anti_aliasing/quality/msaa_3d",
    "taa": "rendering/anti_aliasing/quality/use_taa",
    "use_debanding": "rendering/anti_aliasing/quality/use_debanding",
}
const PHYSICS_SETTINGS := {
    "ticks_per_second": "physics/common/physics_ticks_per_second",
    "max_steps_per_frame": "physics/common/max_physics_steps_per_frame",
    "jitter_fix": "physics/common/physics_jitter_fix",
    "gravity_2d": "physics/2d/default_gravity",
    "gravity_3d": "physics/3d/default_gravity",
    "gravity_vector_3d": "physics/3d/default_gravity_vector",
}

const LAYER_PREFIXES := {
    "2d_render": "layer_names/2d_render/layer_",
    "2d_physics": "layer_names/2d_physics/layer_",
    "3d_render": "layer_names/3d_render/layer_",
    "3d_physics": "layer_names/3d_physics/layer_",
    "navigation": "layer_names/navigation/layer_",
}

static func window_inspect(ctx, _args: Dictionary) -> Dictionary:
    return _inspect_map(ctx, WINDOW_SETTINGS)

static func window_configure(ctx, args: Dictionary) -> Dictionary:
    return _configure_map(ctx, args, WINDOW_SETTINGS)

static func rendering_inspect(ctx, _args: Dictionary) -> Dictionary:
    return _inspect_map(ctx, RENDERING_SETTINGS)

static func rendering_configure(ctx, args: Dictionary) -> Dictionary:
    return _configure_map(ctx, args, RENDERING_SETTINGS)
static func physics_inspect(ctx, _args: Dictionary) -> Dictionary:
    var result := _inspect_map(ctx, PHYSICS_SETTINGS)
    if result.has("_error"):
        return result
    var data: Dictionary = result["data"]
    if data.has("gravity_vector_3d") and data["gravity_vector_3d"] is Vector3:
        var v: Vector3 = data["gravity_vector_3d"]
        data["gravity_vector_3d"] = [v.x, v.y, v.z]
    return result

static func physics_configure(ctx, args: Dictionary) -> Dictionary:
    var normalized := args.duplicate(true)
    if normalized.has("gravity_vector_3d"):
        var v = normalized["gravity_vector_3d"]
        if not (v is Array) or v.size() != 3:
            return ctx._error("invalid_argument", "gravity_vector_3d must contain three numbers")
        normalized["gravity_vector_3d"] = Vector3(float(v[0]), float(v[1]), float(v[2]))
    return _configure_map(ctx, normalized, PHYSICS_SETTINGS)

static func layers_inspect(ctx, _args: Dictionary) -> Dictionary:
    var data := {}
    for kind in LAYER_PREFIXES:
        var rows: Array = []
        for index in range(1, 33):
            var setting: String = LAYER_PREFIXES[kind] + str(index)
            var name := str(ProjectSettings.get_setting(setting, ""))
            if not name.is_empty():
                rows.append({"index": index, "name": name})
        data[kind] = rows
    return {"stamp": ctx._stamp(), "data": data}
static func layer_set(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty():
        return conflict
    var kind := str(args.get("kind", ""))
    var index := int(args.get("index", 0))
    var name := str(args.get("name", ""))
    if not LAYER_PREFIXES.has(kind) or index < 1 or index > 32 or name.length() > 96:
        return ctx._error("invalid_argument", "invalid project layer")
    var setting: String = LAYER_PREFIXES[kind] + str(index)
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [setting], "dry-run")
    ProjectSettings.set_setting(setting, name if not name.is_empty() else null)
    if ProjectSettings.save() != OK:
        return ctx._error("backend_failed", "failed to persist project layer")
    ctx._revision += 1
    return ctx._mutation_result(true, [setting], "Set project layer")

static func autoload_list(ctx, _args: Dictionary) -> Dictionary:
    var rows: Array = []
    for property in ProjectSettings.get_property_list():
        var setting := str(property.get("name", ""))
        if not setting.begins_with("autoload/"):
            continue
        var name := setting.trim_prefix("autoload/")
        var raw := str(ProjectSettings.get_setting(setting, ""))
        rows.append({"name": name, "path": raw.trim_prefix("*"), "singleton": raw.begins_with("*")})
    rows.sort_custom(func(a, b): return str(a["name"]) < str(b["name"]))
    return {"stamp": ctx._stamp(), "data": {"autoloads": rows}}
static func autoload_add(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty():
        return conflict
    var name := str(args.get("name", ""))
    var path := str(args.get("path", ""))
    if name.is_empty() or not name.is_valid_identifier() or not ctx._safe_res(path):
        return ctx._error("invalid_argument", "invalid autoload name or path")
    if not FileAccess.file_exists(path):
        return ctx._error("not_found", "autoload source does not exist")
    if ProjectSettings.has_setting("autoload/" + name):
        return ctx._error("conflict", "autoload already exists")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, ["autoload/" + name], "dry-run")
    ctx.add_autoload_singleton(name, path)
    if ProjectSettings.save() != OK:
        return ctx._error("backend_failed", "failed to persist autoload")
    ctx._revision += 1
    return ctx._mutation_result(true, ["autoload/" + name], "Add autoload")

static func autoload_remove(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty():
        return conflict
    var name := str(args.get("name", ""))
    if name.is_empty() or not ProjectSettings.has_setting("autoload/" + name):
        return ctx._error("not_found", "autoload not found")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, ["autoload/" + name], "dry-run")
    ctx.remove_autoload_singleton(name)
    if ProjectSettings.save() != OK:
        return ctx._error("backend_failed", "failed to persist autoload removal")
    ctx._revision += 1
    return ctx._mutation_result(true, ["autoload/" + name], "Remove autoload")
static func _inspect_map(ctx, mapping: Dictionary) -> Dictionary:
    var data := {}
    for semantic_name in mapping:
        var setting: String = mapping[semantic_name]
        var value = ProjectSettings.get_setting(setting, null)
        data[semantic_name] = value
    return {"stamp": ctx._stamp(), "data": data}

static func _configure_map(ctx, args: Dictionary, mapping: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty():
        return conflict
    var touched: Array[String] = []
    for semantic_name in mapping:
        if args.has(semantic_name):
            touched.append(mapping[semantic_name])
    if touched.is_empty():
        return ctx._error("invalid_argument", "no project settings supplied")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, touched, "dry-run")
    for semantic_name in mapping:
        if args.has(semantic_name):
            ProjectSettings.set_setting(mapping[semantic_name], args[semantic_name])
    if ProjectSettings.save() != OK:
        return ctx._error("backend_failed", "failed to persist project settings")
    ctx._revision += 1
    return ctx._mutation_result(true, touched, "Configure project settings")
