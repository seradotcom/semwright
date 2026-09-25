@tool
extends RefCounted

static func inspect(ctx, args: Dictionary) -> Dictionary:
    var path = str(args.get("path", ""))
    if not ctx._safe_res(path) or not ResourceLoader.exists(path):
        return ctx._error("not_found", "resource not found")
    var resource = ResourceLoader.load(path, "", ResourceLoader.CACHE_MODE_IGNORE)
    if resource == null: return ctx._error("backend_failed", "resource load failed")
    var properties = {}
    for p in resource.get_property_list():
        if int(p.get("usage", 0)) & PROPERTY_USAGE_STORAGE == 0: continue
        var name = str(p.get("name", ""))
        properties[name] = ctx._encode_value(resource.get(name))
    var current_stamp = ctx._stamp()
    return {"stamp":current_stamp,"data":{
        "path":path,
        "class":resource.get_class(),
        "ref":ctx._make_ref("resource", path, resource.get_class(), current_stamp),
        "properties":properties
    }}

static func create(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var path = str(args.get("path", ""))
    var klass = str(args.get("class", ""))
    if not ctx._safe_res(path) or not ClassDB.class_exists(klass) or not ClassDB.is_parent_class(klass, "Resource"):
        return ctx._error("invalid_argument", "invalid resource path/class")
    if ResourceLoader.exists(path): return ctx._error("conflict", "resource already exists")
    if bool(args.get("dry_run", false)): return ctx._mutation_result(false, [path], "dry-run")
    var resource = ClassDB.instantiate(klass)
    if not (resource is Resource): return ctx._error("backend_failed", "resource instantiation failed")
    for patch in args.get("properties", []):
        var result = _apply_resource_property(ctx, resource, patch)
        if not result.is_empty(): return result
    var err = ResourceSaver.save(resource, path)
    if err != OK: return ctx._error("backend_failed", "resource save failed")
    EditorInterface.get_resource_filesystem().update_file(path)
    ctx._revision += 1
    return ctx._mutation_result(true, [path], "Create resource")

static func patch(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var path = str(args.get("path", ""))
    if not ctx._safe_res(path) or not ResourceLoader.exists(path): return ctx._error("not_found", "resource not found")
    if bool(args.get("dry_run", false)): return ctx._mutation_result(false, [path], "dry-run")
    var resource = ResourceLoader.load(path, "", ResourceLoader.CACHE_MODE_IGNORE)
    if resource == null: return ctx._error("backend_failed", "resource load failed")
    for item in args.get("properties", []):
        var result = _apply_resource_property(ctx, resource, item)
        if not result.is_empty(): return result
    var err = ResourceSaver.save(resource, path)
    if err != OK: return ctx._error("backend_failed", "resource save failed")
    EditorInterface.get_resource_filesystem().update_file(path)
    ctx._revision += 1
    return ctx._mutation_result(true, [path], "Patch resource")

static func duplicate_resource(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var source = str(args.get("source", ""))
    var destination = str(args.get("destination", ""))
    if not ctx._safe_res(source) or not ctx._safe_res(destination) or not ResourceLoader.exists(source):
        return ctx._error("not_found", "source resource not found")
    if ResourceLoader.exists(destination): return ctx._error("conflict", "destination already exists")
    if bool(args.get("dry_run", false)): return ctx._mutation_result(false, [destination], "dry-run")
    var resource = ResourceLoader.load(source, "", ResourceLoader.CACHE_MODE_IGNORE)
    if resource == null: return ctx._error("backend_failed", "resource load failed")
    var copy = resource.duplicate(true)
    var err = ResourceSaver.save(copy, destination)
    if err != OK: return ctx._error("backend_failed", "resource duplicate save failed")
    EditorInterface.get_resource_filesystem().update_file(destination)
    ctx._revision += 1
    return ctx._mutation_result(true, [destination], "Duplicate resource")

static func _apply_resource_property(ctx, resource: Resource, item: Dictionary) -> Dictionary:
    var name = str(item.get("name", ""))
    var decoded = ctx._decode_for_property(resource, name, item.get("value"))
    if not bool(decoded.get("ok", false)):
        return ctx._error(
            str(decoded.get("code", "invalid_argument")),
            str(decoded.get("message", "invalid resource property value")),
        )
    resource.set(name, decoded.get("value"))
    return {}
