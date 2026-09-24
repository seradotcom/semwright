@tool
extends RefCounted

const MAX_SHADER_BYTES = 262144

static func physics_layers(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var node = ctx._resolve_node(str(args.get("target", "")))
    if node == null: return ctx._error("not_found", "physics node not found")
    if not (node is CollisionObject2D) and not (node is CollisionObject3D):
        return ctx._error("invalid_argument", "node is not a collision object")
    var layer = int(args.get("layer", node.collision_layer))
    var mask = int(args.get("mask", node.collision_mask))
    if layer < 0 or mask < 0: return ctx._error("invalid_argument", "physics layers must be non-negative")
    if bool(args.get("dry_run", false)): return ctx._mutation_result(false, [str(args.get("target", ""))], "dry-run")
    node.collision_layer = layer
    node.collision_mask = mask
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("target", ""))], "Set physics layers")

static func ui_layout(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var node = ctx._resolve_node(str(args.get("target", "")))
    if not (node is Control): return ctx._error("not_found", "Control node not found")
    if bool(args.get("dry_run", false)): return ctx._mutation_result(false, [str(args.get("target", ""))], "dry-run")
    for key in ["anchor_left","anchor_top","anchor_right","anchor_bottom","offset_left","offset_top","offset_right","offset_bottom","grow_horizontal","grow_vertical","mouse_filter"]:
        if args.has(key): node.set(key, args[key])
    if args.has("minimum_size"):
        node.custom_minimum_size = ctx._decode_value(args["minimum_size"])
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("target", ""))], "Set UI layout")

static func shader_write(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var path = str(args.get("path", ""))
    var code = str(args.get("code", ""))
    if not ctx._safe_res(path) or not path.ends_with(".gdshader"): return ctx._error("invalid_argument", "shader path must be res://*.gdshader")
    if code.to_utf8_buffer().size() > MAX_SHADER_BYTES: return ctx._error("invalid_argument", "shader exceeds size limit")
    if bool(args.get("dry_run", false)): return ctx._mutation_result(false, [path], "dry-run")
    var shader = Shader.new()
    shader.code = code
    var err = ResourceSaver.save(shader, path)
    if err != OK: return ctx._error("invalid_argument", "shader compilation/save failed")
    EditorInterface.get_resource_filesystem().update_file(path)
    ctx._revision += 1
    return ctx._mutation_result(true, [path], "Write shader")

static func shader_attach(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var node = ctx._resolve_node(str(args.get("target", "")))
    var path = str(args.get("path", ""))
    var property = str(args.get("property", "material_override"))
    if node == null or not ctx._safe_res(path) or not ResourceLoader.exists(path): return ctx._error("not_found", "node or shader not found")
    if property not in ["material_override", "material"]: return ctx._error("invalid_argument", "unsupported material property")
    var shader = ResourceLoader.load(path, "Shader", ResourceLoader.CACHE_MODE_REPLACE)
    if not (shader is Shader): return ctx._error("invalid_argument", "resource is not a Shader")
    if bool(args.get("dry_run", false)): return ctx._mutation_result(false, [str(args.get("target", ""))], "dry-run")
    var material = ShaderMaterial.new()
    material.shader = shader
    if not node.has_method("set"): return ctx._error("invalid_argument", "target does not accept material")
    node.set(property, material)
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("target", "")),path], "Attach shader")
