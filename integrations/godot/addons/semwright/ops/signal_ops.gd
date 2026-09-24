@tool
extends RefCounted

static func list(ctx, args: Dictionary) -> Dictionary:
    var node = ctx._resolve_node(str(args.get("target", "")))
    if node == null: return ctx._error("not_found", "target node not found")
    var signals: Array = []
    for signal_info in node.get_signal_list():
        var name = str(signal_info.get("name", ""))
        var connections: Array = []
        for connection in node.get_signal_connection_list(name):
            var callable: Callable = connection.get("callable", Callable())
            connections.append({
                "target": str(callable.get_object_id()),
                "method": str(callable.get_method()),
                "flags": int(connection.get("flags", 0)),
            })
        signals.append({"name":name,"connections":connections})
    return {"stamp":ctx._stamp(),"data":signals}

static func connect_signal(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var source = ctx._resolve_node(str(args.get("source", "")))
    var target = ctx._resolve_node(str(args.get("target", "")))
    var signal_name = str(args.get("signal", ""))
    var method = str(args.get("method", ""))
    if source == null or target == null: return ctx._error("not_found", "signal endpoint node not found")
    if not source.has_signal(signal_name) or method.is_empty():
        return ctx._error("invalid_argument", "unknown signal or empty method")
    var callable = Callable(target, method)
    if source.is_connected(signal_name, callable):
        return ctx._error("conflict", "signal is already connected")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [str(args.get("source", ""))], "dry-run")
    var err = source.connect(signal_name, callable, int(args.get("flags", Object.CONNECT_PERSIST)))
    if err != OK: return ctx._error("backend_failed", "signal connection failed")
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("source", "")),str(args.get("target", ""))], "Connect signal")

static func disconnect_signal(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var source = ctx._resolve_node(str(args.get("source", "")))
    var target = ctx._resolve_node(str(args.get("target", "")))
    var signal_name = str(args.get("signal", ""))
    var method = str(args.get("method", ""))
    if source == null or target == null: return ctx._error("not_found", "signal endpoint node not found")
    var callable = Callable(target, method)
    if not source.is_connected(signal_name, callable): return ctx._error("not_found", "signal connection not found")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [str(args.get("source", ""))], "dry-run")
    source.disconnect(signal_name, callable)
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("source", "")),str(args.get("target", ""))], "Disconnect signal")
