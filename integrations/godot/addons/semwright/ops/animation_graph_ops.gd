@tool
extends RefCounted

const MAX_GRAPH_NODES := 512
const MAX_BLEND_POINTS := 256
const GRAPH_NODE_KINDS := {
    "animation": "AnimationNodeAnimation",
    "state_machine": "AnimationNodeStateMachine",
    "blend_tree": "AnimationNodeBlendTree",
    "blend_space_1d": "AnimationNodeBlendSpace1D",
    "blend_space_2d": "AnimationNodeBlendSpace2D",
    "one_shot": "AnimationNodeOneShot",
    "transition": "AnimationNodeTransition",
    "time_scale": "AnimationNodeTimeScale",
    "time_seek": "AnimationNodeTimeSeek",
    "blend2": "AnimationNodeBlend2",
    "blend3": "AnimationNodeBlend3",
    "add2": "AnimationNodeAdd2",
    "add3": "AnimationNodeAdd3",
    "sub2": "AnimationNodeSub2",
}
const ROOT_NODE_KINDS := {
    "animation": "AnimationNodeAnimation",
    "state_machine": "AnimationNodeStateMachine",
    "blend_tree": "AnimationNodeBlendTree",
    "blend_space_1d": "AnimationNodeBlendSpace1D",
    "blend_space_2d": "AnimationNodeBlendSpace2D",
}

static func node_inspect(ctx, args: Dictionary) -> Dictionary:
    var resolved = _resolve_graph(ctx, args)
    if resolved.has("_error"): return resolved
    return {"stamp":ctx._stamp(),"data":_node_data(resolved["node"], str(args.get("graph", "")))}

static func node_configure(ctx, args: Dictionary) -> Dictionary:
    var resolved = _resolve_graph(ctx, args)
    if resolved.has("_error"): return resolved
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var node: AnimationNode = resolved["node"]
    var error := _validate_node_config(node, args)
    if not error.is_empty(): return ctx._error("invalid_argument", error)
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [str(args.get("graph", ""))], "dry-run")
    _apply_node_config(node, args)
    if args.has("position"):
        var parent = resolved.get("parent")
        var name := StringName(str(resolved.get("name", "")))
        var pos := _v2(args["position"])
        if parent is AnimationNodeStateMachine:
            parent.set_node_position(name, pos)
        elif parent is AnimationNodeBlendTree:
            parent.set_node_position(name, pos)
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("graph", ""))], "Configure AnimationTree graph node")

static func blend_tree_inspect(ctx, args: Dictionary) -> Dictionary:
    var resolved = _resolve_graph(ctx, args)
    if resolved.has("_error"): return resolved
    var blend = resolved["node"] as AnimationNodeBlendTree
    if blend == null: return ctx._error("invalid_argument", "graph target is not AnimationNodeBlendTree")
    var rows: Array = []
    for name in blend.get_node_list():
        if rows.size() >= MAX_GRAPH_NODES: break
        var child: AnimationNode = blend.get_node(name)
        var row := _node_data(child, _child_graph(str(args.get("graph", "")), str(name)))
        row["name"] = str(name)
        row["position"] = _vec2(blend.get_node_position(name))
        var inputs: Array = []
        for i in child.get_input_count():
            inputs.append({"index":i,"name":child.get_input_name(i)})
        row["inputs"] = inputs
        rows.append(row)
    var connections: Array = []
    var raw = blend.get("node_connections")
    if raw is Array:
        for i in range(0, raw.size() - 2, 3):
            connections.append({
                "input_node":str(raw[i]),
                "input_index":int(raw[i + 1]),
                "output_node":str(raw[i + 2]),
            })
    return {"stamp":ctx._stamp(),"data":{
        "graph":str(args.get("graph", "")),
        "graph_offset":_vec2(blend.graph_offset),
        "nodes":rows,
        "connections":connections,
        "truncated":blend.get_node_list().size() > MAX_GRAPH_NODES,
    }}

static func blend_tree_node_add(ctx, args: Dictionary) -> Dictionary:
    var resolved = _resolve_graph(ctx, args)
    if resolved.has("_error"): return resolved
    var blend = resolved["node"] as AnimationNodeBlendTree
    if blend == null: return ctx._error("invalid_argument", "graph target is not AnimationNodeBlendTree")
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var name := str(args.get("name", ""))
    if not _safe_name(name) or name == "output" or blend.has_node(StringName(name)):
        return ctx._error("conflict", "invalid or duplicate BlendTree node name")
    var node = _new_graph_node(str(args.get("kind", "")), args)
    if not (node is AnimationNode): return ctx._error("invalid_argument", "unsupported BlendTree node kind")
    var error := _validate_node_config(node, args)
    if not error.is_empty(): return ctx._error("invalid_argument", error)
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [_child_graph(str(args.get("graph", "")), name)], "dry-run")
    _apply_node_config(node, args)
    blend.add_node(StringName(name), node, _v2(args.get("position", [0.0, 0.0])))
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [_child_graph(str(args.get("graph", "")), name)], "Add BlendTree node")

static func blend_tree_node_configure(ctx, args: Dictionary) -> Dictionary:
    var resolved = _resolve_graph(ctx, args)
    if resolved.has("_error"): return resolved
    var blend = resolved["node"] as AnimationNodeBlendTree
    if blend == null: return ctx._error("invalid_argument", "graph target is not AnimationNodeBlendTree")
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var name := str(args.get("name", ""))
    if name == "output" or not blend.has_node(StringName(name)):
        return ctx._error("not_found", "BlendTree node not found or immutable")
    var node: AnimationNode = blend.get_node(StringName(name))
    var error := _validate_node_config(node, args)
    if not error.is_empty(): return ctx._error("invalid_argument", error)
    var new_name := str(args.get("new_name", ""))
    if not new_name.is_empty() and (not _safe_name(new_name) or new_name == "output" or (new_name != name and blend.has_node(StringName(new_name)))):
        return ctx._error("conflict", "invalid or duplicate BlendTree node rename")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [_child_graph(str(args.get("graph", "")), name)], "dry-run")
    _apply_node_config(node, args)
    if args.has("position"): blend.set_node_position(StringName(name), _v2(args["position"]))
    if not new_name.is_empty() and new_name != name: blend.rename_node(StringName(name), StringName(new_name))
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [_child_graph(str(args.get("graph", "")), new_name if not new_name.is_empty() else name)], "Configure BlendTree node")

static func blend_tree_node_remove(ctx, args: Dictionary) -> Dictionary:
    var resolved = _resolve_graph(ctx, args)
    if resolved.has("_error"): return resolved
    var blend = resolved["node"] as AnimationNodeBlendTree
    if blend == null: return ctx._error("invalid_argument", "graph target is not AnimationNodeBlendTree")
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var name := str(args.get("name", ""))
    if name == "output" or not blend.has_node(StringName(name)):
        return ctx._error("not_found", "BlendTree node not found or immutable")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [_child_graph(str(args.get("graph", "")), name)], "dry-run")
    blend.remove_node(StringName(name))
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [_child_graph(str(args.get("graph", "")), name)], "Remove BlendTree node")

static func blend_tree_connection_set(ctx, args: Dictionary) -> Dictionary:
    var resolved = _resolve_graph(ctx, args)
    if resolved.has("_error"): return resolved
    var blend = resolved["node"] as AnimationNodeBlendTree
    if blend == null: return ctx._error("invalid_argument", "graph target is not AnimationNodeBlendTree")
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var input_node := str(args.get("input_node", ""))
    var output_node := str(args.get("output_node", ""))
    var input_index := int(args.get("input_index", -1))
    if not blend.has_node(StringName(input_node)) or not blend.has_node(StringName(output_node)):
        return ctx._error("not_found", "BlendTree connection endpoint not found")
    var input: AnimationNode = blend.get_node(StringName(input_node))
    if input_index < 0 or input_index >= input.get_input_count():
        return ctx._error("invalid_argument", "BlendTree input index is out of range")
    var existing := _connection_for(blend, input_node, input_index)
    var connected := bool(args.get("connected", true))
    if connected and not existing.is_empty():
        if existing == output_node: return ctx._error("conflict", "BlendTree connection already exists")
        return ctx._error("conflict", "BlendTree input already has a different connection")
    if not connected and existing.is_empty():
        return ctx._error("not_found", "BlendTree connection not found")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [input_node + ":" + str(input_index)], "dry-run")
    if connected:
        # Godot 4.7 exposes connect_node() as void. All rejectable cases are
        # checked above so we never pretend there is a return status to inspect.
        blend.connect_node(StringName(input_node), input_index, StringName(output_node))
    else:
        blend.disconnect_node(StringName(input_node), input_index)
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [input_node + ":" + str(input_index)], "Set BlendTree connection")

static func blend_space_inspect(ctx, args: Dictionary) -> Dictionary:
    var resolved = _resolve_graph(ctx, args)
    if resolved.has("_error"): return resolved
    var space = resolved["node"]
    if not (space is AnimationNodeBlendSpace1D) and not (space is AnimationNodeBlendSpace2D):
        return ctx._error("invalid_argument", "graph target is not a blend space")
    var points: Array = []
    var count: int = space.get_blend_point_count()
    for i in mini(count, MAX_BLEND_POINTS):
        var child: AnimationRootNode = space.get_blend_point_node(i)
        points.append({
            "index":i,
            "name":str(space.get_blend_point_name(i)),
            "position":space.get_blend_point_position(i) if space is AnimationNodeBlendSpace1D else _vec2(space.get_blend_point_position(i)),
            "node":_node_data(child, _child_graph(str(args.get("graph", "")), str(space.get_blend_point_name(i)))),
        })
    var data := {
        "graph":str(args.get("graph", "")),
        "dimension":1 if space is AnimationNodeBlendSpace1D else 2,
        "blend_mode":space.blend_mode,
        "sync_mode":space.sync_mode,
        "cyclic_length":space.cyclic_length,
        "points":points,
        "truncated":count > MAX_BLEND_POINTS,
    }
    if space is AnimationNodeBlendSpace1D:
        data["min_space"] = space.min_space
        data["max_space"] = space.max_space
        data["snap"] = space.snap
        data["value_label"] = space.value_label
        data["triangles"] = []
        data["auto_triangles"] = false
    else:
        data["min_space"] = _vec2(space.min_space)
        data["max_space"] = _vec2(space.max_space)
        data["snap"] = _vec2(space.snap)
        data["x_label"] = space.x_label
        data["y_label"] = space.y_label
        data["auto_triangles"] = space.auto_triangles
        var triangles: Array = []
        for i in space.get_triangle_count():
            triangles.append([
                space.get_triangle_point(i, 0),
                space.get_triangle_point(i, 1),
                space.get_triangle_point(i, 2),
            ])
        data["triangles"] = triangles
    return {"stamp":ctx._stamp(),"data":data}

static func blend_space_configure(ctx, args: Dictionary) -> Dictionary:
    var resolved = _resolve_graph(ctx, args)
    if resolved.has("_error"): return resolved
    var space = resolved["node"]
    if not (space is AnimationNodeBlendSpace1D) and not (space is AnimationNodeBlendSpace2D):
        return ctx._error("invalid_argument", "graph target is not a blend space")
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var error := _validate_blend_space(space, args)
    if not error.is_empty(): return ctx._error("invalid_argument", error)
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [str(args.get("graph", ""))], "dry-run")
    _apply_blend_space(space, args)
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("graph", ""))], "Configure AnimationTree blend space")

static func blend_space_point_add(ctx, args: Dictionary) -> Dictionary:
    var resolved = _resolve_graph(ctx, args)
    if resolved.has("_error"): return resolved
    var space = resolved["node"]
    if not (space is AnimationNodeBlendSpace1D) and not (space is AnimationNodeBlendSpace2D):
        return ctx._error("invalid_argument", "graph target is not a blend space")
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    if space.get_blend_point_count() >= MAX_BLEND_POINTS:
        return ctx._error("invalid_argument", "blend-point limit reached")
    var name := str(args.get("name", ""))
    if not _safe_name(name) or space.find_blend_point_by_name(StringName(name)) >= 0:
        return ctx._error("conflict", "invalid or duplicate blend-point name")
    var node = _new_root_node(str(args.get("kind", "")), args)
    if not (node is AnimationRootNode): return ctx._error("invalid_argument", "unsupported blend-point node kind")
    var index := int(args.get("index", -1))
    if index < -1 or index > space.get_blend_point_count():
        return ctx._error("invalid_argument", "blend-point insertion index is out of range")
    var position = _blend_position(space, args.get("position"))
    if position == null: return ctx._error("invalid_argument", "blend-point position has wrong dimension")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [_child_graph(str(args.get("graph", "")), name)], "dry-run")
    if space is AnimationNodeBlendSpace1D:
        space.add_blend_point(node, float(position), index, StringName(name))
    else:
        space.add_blend_point(node, position, index, StringName(name))
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [_child_graph(str(args.get("graph", "")), name)], "Add AnimationTree blend point")

static func blend_space_point_configure(ctx, args: Dictionary) -> Dictionary:
    var resolved = _resolve_graph(ctx, args)
    if resolved.has("_error"): return resolved
    var space = resolved["node"]
    if not (space is AnimationNodeBlendSpace1D) and not (space is AnimationNodeBlendSpace2D):
        return ctx._error("invalid_argument", "graph target is not a blend space")
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var index := int(args.get("index", -1))
    if index < 0 or index >= space.get_blend_point_count():
        return ctx._error("not_found", "blend point not found")
    var name := str(args.get("name", ""))
    if not name.is_empty():
        var existing := space.find_blend_point_by_name(StringName(name))
        if not _safe_name(name) or (existing >= 0 and existing != index):
            return ctx._error("conflict", "invalid or duplicate blend-point name")
    var position = null
    if args.has("position"):
        position = _blend_position(space, args["position"])
        if position == null: return ctx._error("invalid_argument", "blend-point position has wrong dimension")
    var node: AnimationRootNode = space.get_blend_point_node(index)
    var error := _validate_node_config(node, args)
    if not error.is_empty(): return ctx._error("invalid_argument", error)
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [_child_graph(str(args.get("graph", "")), str(space.get_blend_point_name(index)))], "dry-run")
    if position != null: space.set_blend_point_position(index, position)
    if not name.is_empty(): space.set_blend_point_name(index, StringName(name))
    _apply_node_config(node, args)
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("graph", "")), "point:%d" % index], "Configure AnimationTree blend point")

static func blend_space_point_remove(ctx, args: Dictionary) -> Dictionary:
    var resolved = _resolve_graph(ctx, args)
    if resolved.has("_error"): return resolved
    var space = resolved["node"]
    if not (space is AnimationNodeBlendSpace1D) and not (space is AnimationNodeBlendSpace2D):
        return ctx._error("invalid_argument", "graph target is not a blend space")
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var index := int(args.get("index", -1))
    if index < 0 or index >= space.get_blend_point_count():
        return ctx._error("not_found", "blend point not found")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [str(args.get("graph", "")), "point:%d" % index], "dry-run")
    space.remove_blend_point(index)
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("graph", "")), "point:%d" % index], "Remove AnimationTree blend point")

static func blend_space_triangle_add(ctx, args: Dictionary) -> Dictionary:
    var resolved = _resolve_graph(ctx, args)
    if resolved.has("_error"): return resolved
    var space = resolved["node"] as AnimationNodeBlendSpace2D
    if space == null: return ctx._error("invalid_argument", "triangle authoring requires BlendSpace2D")
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    if space.auto_triangles: return ctx._error("conflict", "disable auto_triangles before manual triangle authoring")
    var indices := [int(args.get("a", -1)), int(args.get("b", -1)), int(args.get("c", -1))]
    if indices[0] == indices[1] or indices[0] == indices[2] or indices[1] == indices[2]:
        return ctx._error("invalid_argument", "blend-space triangle points must be distinct")
    for index in indices:
        if index < 0 or index >= space.get_blend_point_count():
            return ctx._error("invalid_argument", "blend-space triangle point is out of range")
    var at_index := int(args.get("index", -1))
    if at_index < -1 or at_index > space.get_triangle_count():
        return ctx._error("invalid_argument", "triangle insertion index is out of range")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [str(args.get("graph", ""))], "dry-run")
    space.add_triangle(indices[0], indices[1], indices[2], at_index)
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("graph", ""))], "Add AnimationTree blend-space triangle")

static func blend_space_triangle_remove(ctx, args: Dictionary) -> Dictionary:
    var resolved = _resolve_graph(ctx, args)
    if resolved.has("_error"): return resolved
    var space = resolved["node"] as AnimationNodeBlendSpace2D
    if space == null: return ctx._error("invalid_argument", "triangle authoring requires BlendSpace2D")
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    if space.auto_triangles: return ctx._error("conflict", "disable auto_triangles before manual triangle authoring")
    var index := int(args.get("index", -1))
    if index < 0 or index >= space.get_triangle_count():
        return ctx._error("not_found", "blend-space triangle not found")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [str(args.get("graph", "")), "triangle:%d" % index], "dry-run")
    space.remove_triangle(index)
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("graph", "")), "triangle:%d" % index], "Remove AnimationTree blend-space triangle")

static func _resolve_graph(ctx, args: Dictionary) -> Dictionary:
    var tree = ctx._resolve_node(str(args.get("tree", "")))
    if not (tree is AnimationTree):
        return ctx._error("not_found", "AnimationTree not found")
    if tree.tree_root == null:
        return ctx._error("not_found", "AnimationTree has no root node")
    var graph := str(args.get("graph", ""))
    if graph.length() > 1024 or graph.begins_with("/") or graph.contains(".."):
        return ctx._error("invalid_argument", "invalid animation graph path")
    var current: AnimationNode = tree.tree_root
    var parent = null
    var name := ""
    if not graph.is_empty():
        for raw_segment in graph.split("/", false):
            var segment := str(raw_segment)
            if not _safe_name(segment):
                return ctx._error("invalid_argument", "invalid animation graph segment")
            parent = current
            name = segment
            if current is AnimationNodeStateMachine:
                if not current.has_node(StringName(segment)):
                    return ctx._error("not_found", "animation graph state not found")
                current = current.get_node(StringName(segment))
            elif current is AnimationNodeBlendTree:
                if not current.has_node(StringName(segment)):
                    return ctx._error("not_found", "animation blend-tree node not found")
                current = current.get_node(StringName(segment))
            elif current is AnimationNodeBlendSpace1D or current is AnimationNodeBlendSpace2D:
                var index: int = current.find_blend_point_by_name(StringName(segment))
                if index < 0: return ctx._error("not_found", "animation blend point not found")
                current = current.get_blend_point_node(index)
            else:
                return ctx._error("invalid_argument", "animation graph path crosses a leaf node")
    return {"tree":tree,"node":current,"parent":parent,"name":name}

static func _node_data(node: AnimationNode, graph: String) -> Dictionary:
    var data := {"graph":graph,"class":node.get_class(),"input_count":node.get_input_count()}
    if node is AnimationNodeAnimation:
        data["animation"] = str(node.animation)
    if node is AnimationNodeSync:
        data["sync"] = node.sync
    if node is AnimationNodeOneShot:
        data.merge({
            "fadein_time":node.fadein_time,"fadeout_time":node.fadeout_time,
            "mix_mode":node.mix_mode,"autorestart":node.autorestart,
            "autorestart_delay":node.autorestart_delay,
            "autorestart_random_delay":node.autorestart_random_delay,
            "abort_on_reset":node.abort_on_reset,"break_loop_at_end":node.break_loop_at_end,
        })
    if node is AnimationNodeTransition:
        var inputs: Array = []
        for i in node.input_count:
            inputs.append({
                "index":i,"name":node.get_input_name(i),
                "auto_advance":node.is_input_set_as_auto_advance(i),
                "reset":node.is_input_reset(i),
                "break_loop_at_end":node.is_input_loop_broken_at_end(i),
            })
        data.merge({
            "input_count":node.input_count,"xfade_time":node.xfade_time,
            "allow_transition_to_self":node.allow_transition_to_self,
            "inputs":inputs,
        })
    if node is AnimationNodeBlendTree:
        data["graph_offset"] = _vec2(node.graph_offset)
    if node is AnimationNodeBlendSpace1D:
        data.merge(_blend_space_properties(node))
    if node is AnimationNodeBlendSpace2D:
        data.merge(_blend_space_properties(node))
    return data

static func _validate_node_config(node: AnimationNode, args: Dictionary) -> String:
    if args.has("position") and _v2_or_null(args["position"]) == null:
        return "position must be a 2-number array"
    if node is AnimationNodeOneShot:
        for key in ["fadein_time","fadeout_time","autorestart_delay","autorestart_random_delay"]:
            if args.has(key) and float(args[key]) < 0.0: return "%s must be non-negative" % key
    if node is AnimationNodeTransition:
        if args.has("input_count") and (int(args["input_count"]) < 0 or int(args["input_count"]) > 64):
            return "transition input_count must be 0..64"
        if args.has("xfade_time") and float(args["xfade_time"]) < 0.0:
            return "transition xfade_time must be non-negative"
    if node is AnimationNodeBlendSpace1D or node is AnimationNodeBlendSpace2D:
        return _validate_blend_space(node, args)
    return ""

static func _apply_node_config(node: AnimationNode, args: Dictionary) -> void:
    if node is AnimationNodeAnimation and args.has("animation"):
        node.animation = StringName(str(args["animation"]))
    if node is AnimationNodeSync and args.has("sync"):
        node.sync = bool(args["sync"])
    if node is AnimationNodeOneShot:
        for key in ["fadein_time","fadeout_time","autorestart_delay","autorestart_random_delay"]:
            if args.has(key): node.set(key, float(args[key]))
        for key in ["autorestart","abort_on_reset","break_loop_at_end"]:
            if args.has(key): node.set(key, bool(args[key]))
        if args.has("mix_mode"): node.mix_mode = int(args["mix_mode"])
    if node is AnimationNodeTransition:
        if args.has("input_count"): node.input_count = int(args["input_count"])
        if args.has("xfade_time"): node.xfade_time = float(args["xfade_time"])
        if args.has("allow_transition_to_self"): node.allow_transition_to_self = bool(args["allow_transition_to_self"])
        if args.has("transition_input"):
            var spec: Dictionary = args["transition_input"]
            var index := int(spec.get("index", -1))
            if index >= 0 and index < node.input_count:
                if spec.has("auto_advance"): node.set_input_as_auto_advance(index, bool(spec["auto_advance"]))
                if spec.has("reset"): node.set_input_reset(index, bool(spec["reset"]))
                if spec.has("break_loop_at_end"): node.set_input_break_loop_at_end(index, bool(spec["break_loop_at_end"]))
    if node is AnimationNodeBlendTree and args.has("graph_offset"):
        node.graph_offset = _v2(args["graph_offset"])
    if node is AnimationNodeBlendSpace1D or node is AnimationNodeBlendSpace2D:
        _apply_blend_space(node, args)

static func _validate_blend_space(space, args: Dictionary) -> String:
    if space is AnimationNodeBlendSpace1D:
        var minimum := float(args.get("min_space", space.min_space))
        var maximum := float(args.get("max_space", space.max_space))
        if minimum >= maximum: return "blend-space min_space must be below max_space"
        if args.has("snap") and float(args["snap"]) <= 0.0: return "blend-space snap must be positive"
        if args.has("position") and not (args["position"] is float or args["position"] is int):
            return "1D blend-space position must be numeric"
    else:
        if args.has("min_space") and _v2_or_null(args["min_space"]) == null: return "min_space must be Vector2"
        if args.has("max_space") and _v2_or_null(args["max_space"]) == null: return "max_space must be Vector2"
        if args.has("snap") and _v2_or_null(args["snap"]) == null: return "snap must be Vector2"
        var minimum: Vector2 = _v2(args.get("min_space", _vec2(space.min_space)))
        var maximum: Vector2 = _v2(args.get("max_space", _vec2(space.max_space)))
        if minimum.x >= maximum.x or minimum.y >= maximum.y: return "blend-space min_space must be below max_space"
        if args.has("snap"):
            var snap: Vector2 = _v2(args["snap"])
            if snap.x <= 0.0 or snap.y <= 0.0: return "blend-space snap must be positive"
    if args.has("cyclic_length") and float(args["cyclic_length"]) < 0.0:
        return "cyclic_length must be non-negative"
    return ""

static func _apply_blend_space(space, args: Dictionary) -> void:
    if args.has("blend_mode"): space.blend_mode = int(args["blend_mode"])
    if args.has("sync_mode"): space.sync_mode = int(args["sync_mode"])
    if args.has("cyclic_length"): space.cyclic_length = float(args["cyclic_length"])
    if space is AnimationNodeBlendSpace1D:
        if args.has("min_space"): space.min_space = float(args["min_space"])
        if args.has("max_space"): space.max_space = float(args["max_space"])
        if args.has("snap"): space.snap = float(args["snap"])
        if args.has("value_label"): space.value_label = str(args["value_label"])
    else:
        if args.has("min_space"): space.min_space = _v2(args["min_space"])
        if args.has("max_space"): space.max_space = _v2(args["max_space"])
        if args.has("snap"): space.snap = _v2(args["snap"])
        if args.has("x_label"): space.x_label = str(args["x_label"])
        if args.has("y_label"): space.y_label = str(args["y_label"])
        if args.has("auto_triangles"): space.auto_triangles = bool(args["auto_triangles"])

static func _blend_space_properties(space) -> Dictionary:
    var data := {
        "blend_mode":space.blend_mode,"sync_mode":space.sync_mode,
        "cyclic_length":space.cyclic_length,
    }
    if space is AnimationNodeBlendSpace1D:
        data.merge({
            "dimension":1,"min_space":space.min_space,"max_space":space.max_space,
            "snap":space.snap,"value_label":space.value_label,
        })
    else:
        data.merge({
            "dimension":2,"min_space":_vec2(space.min_space),"max_space":_vec2(space.max_space),
            "snap":_vec2(space.snap),"x_label":space.x_label,"y_label":space.y_label,
            "auto_triangles":space.auto_triangles,
        })
    return data

static func _new_graph_node(kind: String, args: Dictionary):
    if not GRAPH_NODE_KINDS.has(kind): return null
    var node = ClassDB.instantiate(GRAPH_NODE_KINDS[kind])
    if node is AnimationNodeAnimation and args.has("animation"):
        node.animation = StringName(str(args["animation"]))
    return node

static func _new_root_node(kind: String, args: Dictionary):
    if not ROOT_NODE_KINDS.has(kind): return null
    var node = ClassDB.instantiate(ROOT_NODE_KINDS[kind])
    if node is AnimationNodeAnimation and args.has("animation"):
        node.animation = StringName(str(args["animation"]))
    return node
static func _blend_position(space, value):
    if space is AnimationNodeBlendSpace1D:
        if value is int or value is float: return float(value)
        return null
    return _v2_or_null(value)

static func _connection_for(blend: AnimationNodeBlendTree, input_node: String, input_index: int) -> String:
    var raw = blend.get("node_connections")
    if not (raw is Array): return ""
    for i in range(0, raw.size() - 2, 3):
        if str(raw[i]) == input_node and int(raw[i + 1]) == input_index:
            return str(raw[i + 2])
    return ""

static func _safe_name(name: String) -> bool:
    return not name.is_empty() and name.length() <= 96 and not name.contains("/") and not name.contains("..")

static func _child_graph(parent: String, child: String) -> String:
    return child if parent.is_empty() else parent + "/" + child

static func _v2(value) -> Vector2:
    return Vector2(float(value[0]), float(value[1]))

static func _v2_or_null(value):
    if not (value is Array) or value.size() != 2: return null
    return _v2(value)

static func _vec2(value: Vector2) -> Array:
    return [value.x, value.y]
