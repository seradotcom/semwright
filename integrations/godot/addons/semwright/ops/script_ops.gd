@tool
extends RefCounted

const MAX_SCRIPT_BYTES = 262144

static func inspect(ctx, args: Dictionary) -> Dictionary:
    var path = str(args.get("path", ""))
    if not _safe_script(ctx, path) or not FileAccess.file_exists(path): return ctx._error("not_found", "managed script not found")
    var source = FileAccess.get_file_as_string(path)
    if source.to_utf8_buffer().size() > MAX_SCRIPT_BYTES: return ctx._error("backend_failed", "script exceeds inspection budget")
    return {"stamp":ctx._stamp(),"data":{"path":path,"sha256":_sha(source),"source":source}}

static func write(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var path = str(args.get("path", ""))
    var source = str(args.get("source", ""))
    if not _safe_script(ctx, path): return ctx._error("invalid_argument", "script path must be res://*.gd outside addons")
    if source.to_utf8_buffer().size() > MAX_SCRIPT_BYTES: return ctx._error("invalid_argument", "script exceeds size limit")
    if _contains_tool_annotation(source): return ctx._error("permission_denied", "@tool scripts are not allowed through managed script writes")
    var expected = str(args.get("expected_sha256", ""))
    if FileAccess.file_exists(path) and not expected.is_empty():
        var current = FileAccess.get_file_as_string(path)
        if _sha(current) != expected: return ctx._error("conflict", "script changed since inspection")
    elif FileAccess.file_exists(path) and expected.is_empty():
        return ctx._error("conflict", "overwriting an existing script requires expected_sha256")
    if bool(args.get("dry_run", false)): return ctx._mutation_result(false, [path], "dry-run")
    var script = GDScript.new()
    script.source_code = source
    var parse = script.reload()
    if parse != OK: return ctx._error("invalid_argument", "GDScript parser rejected source")
    var temp = path + ".semwright-tmp"
    var file = FileAccess.open(temp, FileAccess.WRITE)
    if file == null: return ctx._error("backend_failed", "failed to create script temp file")
    file.store_string(source)
    file.flush()
    file.close()
    if FileAccess.file_exists(path): DirAccess.remove_absolute(ProjectSettings.globalize_path(path))
    var rename = DirAccess.rename_absolute(ProjectSettings.globalize_path(temp), ProjectSettings.globalize_path(path))
    if rename != OK: return ctx._error("backend_failed", "atomic script rename failed")
    EditorInterface.get_resource_filesystem().update_file(path)
    ctx._revision += 1
    return ctx._mutation_result(true, [path], "Write managed script")

static func attach(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var node = ctx._resolve_node(str(args.get("target", "")))
    var path = str(args.get("path", ""))
    if node == null: return ctx._error("not_found", "target node not found")
    if not _safe_script(ctx, path) or not FileAccess.file_exists(path): return ctx._error("not_found", "managed script not found")
    var source = FileAccess.get_file_as_string(path)
    if _contains_tool_annotation(source): return ctx._error("permission_denied", "@tool script attachment is blocked")
    if bool(args.get("dry_run", false)): return ctx._mutation_result(false, [str(args.get("target", ""))], "dry-run")
    var script = ResourceLoader.load(path, "Script", ResourceLoader.CACHE_MODE_REPLACE)
    if not (script is Script): return ctx._error("invalid_argument", "resource is not a script")
    node.set_script(script)
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("target", "")), path], "Attach managed script")

static func detach(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var node = ctx._resolve_node(str(args.get("target", "")))
    if node == null: return ctx._error("not_found", "target node not found")
    if bool(args.get("dry_run", false)): return ctx._mutation_result(false, [str(args.get("target", ""))], "dry-run")
    node.set_script(null)
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("target", ""))], "Detach script")

static func _safe_script(ctx, path: String) -> bool:
    return ctx._safe_res(path) and path.ends_with(".gd") and not path.begins_with("res://addons/")

static func _contains_tool_annotation(source: String) -> bool:
    for line in source.split("\n"):
        if line.strip_edges().begins_with("@tool"): return true
    return false

static func _sha(source: String) -> String:
    var h = HashingContext.new()
    h.start(HashingContext.HASH_SHA256)
    h.update(source.to_utf8_buffer())
    return h.finish().hex_encode()
