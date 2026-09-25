@tool
extends RefCounted

static func issue_node(ctx, args: Dictionary) -> Dictionary:
    var path := str(args.get("path", ""))
    var node = ctx._resolve_node(path)
    if node == null:
        return ctx._error("not_found", "node reference target not found")
    return {"stamp": ctx._stamp(), "data": {"ref": ctx._make_ref(
        "node", path, node.get_class()
    )}}

static func issue_resource(ctx, args: Dictionary) -> Dictionary:
    var path := str(args.get("path", ""))
    if not ctx._safe_res(path) or not ResourceLoader.exists(path):
        return ctx._error("not_found", "resource reference target not found")
    var resource = ResourceLoader.load(path, "", ResourceLoader.CACHE_MODE_REUSE)
    if resource == null:
        return ctx._error("backend_failed", "resource reference target failed to load")
    return {"stamp": ctx._stamp(), "data": {"ref": ctx._make_ref(
        "resource", path, resource.get_class()
    )}}

static func issue_scene(ctx, _args: Dictionary) -> Dictionary:
    var root := EditorInterface.get_edited_scene_root()
    if root == null:
        return ctx._error("not_found", "no edited scene")
    return {"stamp": ctx._stamp(), "data": {"ref": ctx._make_ref(
        "scene", str(root.scene_file_path), root.get_class()
    )}}
static func resolve(ctx, args: Dictionary) -> Dictionary:
    var ref = args.get("ref", {})
    if not (ref is Dictionary):
        return ctx._error("invalid_argument", "ref must be an object")
    var identity_error := _validate_identity(ctx, ref)
    if not identity_error.is_empty():
        return identity_error
    var kind := str(ref.get("kind", ""))
    var path := str(ref.get("path", ""))
    var expected_class := str(ref.get("class", ""))
    var actual_class := ""
    var exists := false
    match kind:
        "node":
            var node = ctx._resolve_node(path)
            exists = node != null
            if node != null:
                actual_class = node.get_class()
        "resource":
            if ctx._safe_res(path) and ResourceLoader.exists(path):
                var resource = ResourceLoader.load(path, "", ResourceLoader.CACHE_MODE_REUSE)
                exists = resource != null
                if resource != null:
                    actual_class = resource.get_class()
        "scene":
            var root := EditorInterface.get_edited_scene_root()
            exists = root != null and str(root.scene_file_path) == path
            if exists:
                actual_class = root.get_class()
        _:
            return ctx._error("invalid_argument", "unsupported Godot ref kind")
    if not exists:
        return ctx._error("stale_reference", "Godot ref target no longer exists")
    if not expected_class.is_empty() and expected_class != actual_class:
        return ctx._error("stale_reference", "Godot ref class changed")
    var current := ctx._make_ref(kind, path, actual_class)
    var stale := int(ref.get("revision", -1)) != int(current["revision"])         or str(ref.get("fingerprint", "")) != str(current["fingerprint"])
    if bool(args.get("require_current", false)) and stale:
        return ctx._error("stale_reference", "Godot ref revision is stale")
    return {"stamp": ctx._stamp(), "data": {
        "ref": current,
        "stale": stale,
        "previous_revision": int(ref.get("revision", -1)),
    }}

static func _validate_identity(ctx, ref: Dictionary) -> Dictionary:
    if str(ref.get("provider", "")) != "godot":
        return ctx._error("invalid_argument", "ref provider is not Godot")
    if str(ref.get("project", "")) != ctx._project:
        return ctx._error("permission_denied", "Godot ref belongs to another project")
    if str(ref.get("session", "")) != ctx._session:
        return ctx._error("stale_reference", "Godot ref belongs to another session")
    if str(ref.get("generation", "")) != ctx._generation:
        return ctx._error("stale_reference", "Godot ref generation is stale")
    return {}
