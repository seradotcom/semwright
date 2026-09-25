@tool
extends RefCounted

const MAX_SELECTION := 256

static func state(ctx, _args: Dictionary) -> Dictionary:
    var root := EditorInterface.get_edited_scene_root()
    var selected: Array = []
    for node in EditorInterface.get_selection().get_selected_nodes():
        if selected.size() >= MAX_SELECTION:
            break
        if root != null and node is Node:
            selected.append(str(root.get_path_to(node)))
    return {"stamp": ctx._stamp(), "data": {
        "scene": "" if root == null else str(root.scene_file_path),
        "scene_class": "" if root == null else root.get_class(),
        "unsaved_scenes": Array(EditorInterface.get_unsaved_scenes()),
        "playing": EditorInterface.is_playing_scene(),
        "selected_nodes": selected,
        "selected_files": Array(EditorInterface.get_selected_paths()),
    }}

static func selection_get(ctx, _args: Dictionary) -> Dictionary:
    var root := EditorInterface.get_edited_scene_root()
    var rows: Array = []
    if root != null:
        for node in EditorInterface.get_selection().get_selected_nodes():
            if rows.size() >= MAX_SELECTION:
                break
            if node is Node:
                rows.append({"path": str(root.get_path_to(node)), "class": node.get_class(), "name": node.name})
    return {"stamp": ctx._stamp(), "data": {"nodes": rows}}

static func selection_set(ctx, args: Dictionary) -> Dictionary:
    var root := EditorInterface.get_edited_scene_root()
    if root == null:
        return ctx._error("not_found", "no edited scene")
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty():
        return conflict
    var nodes: Array[Node] = []
    for path in args.get("nodes", []):
        var node = ctx._resolve_node(str(path))
        if node == null:
            return ctx._error("not_found", "selection node not found")
        nodes.append(node)
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, Array(args.get("nodes", [])), "dry-run")
    var selection := EditorInterface.get_selection()
    selection.clear()
    for node in nodes:
        selection.add_node(node)
    ctx._revision += 1
    return ctx._mutation_result(true, Array(args.get("nodes", [])), "Set editor selection")

static func run_start(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty():
        return conflict
    if EditorInterface.is_playing_scene():
        return ctx._error("conflict", "editor is already playing a scene")
    var mode := str(args.get("mode", "main"))
    var affected: Array[String] = [mode]
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, affected, "dry-run")
    match mode:
        "main":
            EditorInterface.play_main_scene()
        "current":
            if EditorInterface.get_edited_scene_root() == null:
                return ctx._error("not_found", "no current scene")
            EditorInterface.play_current_scene()
        "custom":
            var path := str(args.get("scene", ""))
            if not ctx._safe_res(path) or not path.ends_with(".tscn") or not ResourceLoader.exists(path):
                return ctx._error("not_found", "custom scene does not exist")
            EditorInterface.play_custom_scene(path)
            affected = [path]
        _:
            return ctx._error("invalid_argument", "unsupported editor run mode")
    ctx._revision += 1
    return ctx._mutation_result(true, affected, "Start editor scene")

static func run_stop(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty():
        return conflict
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, ["runtime"], "dry-run")
    if EditorInterface.is_playing_scene():
        EditorInterface.stop_playing_scene()
    ctx._revision += 1
    return ctx._mutation_result(true, ["runtime"], "Stop editor scene")
