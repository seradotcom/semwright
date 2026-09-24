@tool
extends RefCounted

static func scene_reload(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var root = EditorInterface.get_edited_scene_root()
    if root == null or str(root.scene_file_path).is_empty():
        return ctx._error("not_found", "no saved scene to reload")
    var path = str(root.scene_file_path)
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [path], "dry-run")
    EditorInterface.reload_scene_from_path(path)
    ctx._revision += 1
    return ctx._mutation_result(true, [path], "Reload scene")

static func scene_instantiate(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var path = str(args.get("scene", ""))
    if not ctx._safe_res(path) or not ResourceLoader.exists(path):
        return ctx._error("not_found", "PackedScene does not exist")
    var parent = ctx._resolve_node(str(args.get("parent", ".")))
    if parent == null:
        return ctx._error("not_found", "parent node not found")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [path], "dry-run")
    var packed = ResourceLoader.load(path, "PackedScene", ResourceLoader.CACHE_MODE_REUSE)
    if not (packed is PackedScene):
        return ctx._error("invalid_argument", "resource is not a PackedScene")
    var node = packed.instantiate(PackedScene.GEN_EDIT_STATE_INSTANCE)
    if node == null:
        return ctx._error("backend_failed", "PackedScene instantiation failed")
    var requested_name = str(args.get("name", ""))
    if not requested_name.is_empty(): node.name = requested_name
    parent.add_child(node)
    var root = EditorInterface.get_edited_scene_root()
    node.owner = root
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(root.get_path_to(node))], "Instantiate scene")

static func node_rename(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var node = ctx._resolve_node(str(args.get("target", "")))
    if node == null: return ctx._error("not_found", "target node not found")
    var name = str(args.get("name", ""))
    if name.is_empty() or name.length() > 96 or name.contains("/"):
        return ctx._error("invalid_argument", "invalid node name")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [str(args.get("target", ""))], "dry-run")
    node.name = name
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(EditorInterface.get_edited_scene_root().get_path_to(node))], "Rename node")

static func node_reparent(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var node = ctx._resolve_node(str(args.get("target", "")))
    var parent = ctx._resolve_node(str(args.get("parent", "")))
    var root = EditorInterface.get_edited_scene_root()
    if node == null or parent == null or node == root or node == parent or node.is_ancestor_of(parent):
        return ctx._error("invalid_argument", "invalid reparent operation")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [str(args.get("target", ""))], "dry-run")
    node.reparent(parent, bool(args.get("keep_global_transform", true)))
    node.owner = root
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(root.get_path_to(node))], "Reparent node")

static func group_set(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var node = ctx._resolve_node(str(args.get("target", "")))
    if node == null: return ctx._error("not_found", "target node not found")
    var group = str(args.get("group", ""))
    if group.is_empty() or group.length() > 96: return ctx._error("invalid_argument", "invalid group")
    var enabled = bool(args.get("enabled", true))
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [str(args.get("target", ""))], "dry-run")
    if enabled: node.add_to_group(group, bool(args.get("persistent", true)))
    else: node.remove_from_group(group)
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("target", ""))], "Set group membership")

static func input_remove(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var name = str(args.get("name", ""))
    var setting_name = "input/" + name
    if name.begins_with("ui_") or not ProjectSettings.has_setting(setting_name):
        return ctx._error("not_found", "input action not found or protected")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [setting_name], "dry-run")
    ProjectSettings.set_setting(setting_name, null)
    var save_error = ProjectSettings.save()
    if save_error != OK:
        return ctx._error("backend_failed", "failed to persist input action removal")
    ctx._revision += 1
    return ctx._mutation_result(true, ["input/" + name], "Remove input action")

static func assets_status(ctx, _args: Dictionary) -> Dictionary:
    var fs = EditorInterface.get_resource_filesystem()
    return {"stamp":ctx._stamp(),"data":{"scanning":fs.is_scanning(),"importing":fs.is_importing(),"progress":clampf(fs.get_scanning_progress(), 0.0, 1.0)}}

static func assets_rescan(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var fs = EditorInterface.get_resource_filesystem()
    if fs.is_scanning() or fs.is_importing(): return ctx._error("conflict", "resource filesystem is already busy")
    if bool(args.get("dry_run", false)): return ctx._mutation_result(false, ["res://"], "dry-run")
    fs.scan()
    ctx._revision += 1
    return ctx._mutation_result(true, ["res://"], "Rescan assets")

static func main_scene(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var path = str(args.get("path", ""))
    if not ctx._safe_res(path) or not path.ends_with(".tscn") or not ResourceLoader.exists(path):
        return ctx._error("not_found", "main scene must be an existing res:// .tscn")
    if bool(args.get("dry_run", false)): return ctx._mutation_result(false, [path], "dry-run")
    ProjectSettings.set_setting("application/run/main_scene", path)
    var err = ProjectSettings.save()
    if err != OK: return ctx._error("backend_failed", "failed to persist project settings")
    ctx._revision += 1
    return ctx._mutation_result(true, [path], "Set main scene")
