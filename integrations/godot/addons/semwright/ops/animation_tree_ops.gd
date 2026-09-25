@tool
extends RefCounted

const NODE_KINDS := {
    "animation": "AnimationNodeAnimation",
    "state_machine": "AnimationNodeStateMachine",
    "blend_tree": "AnimationNodeBlendTree",
    "blend_space_1d": "AnimationNodeBlendSpace1D",
    "blend_space_2d": "AnimationNodeBlendSpace2D",
    "one_shot": "AnimationNodeOneShot",
    "transition": "AnimationNodeTransition",
    "time_scale": "AnimationNodeTimeScale",
    "sync": "AnimationNodeSync",
}

static func inspect(ctx, args: Dictionary) -> Dictionary:
    var resolved = _state_machine(ctx, args)
    if resolved.has("_error"):
        return resolved
    var machine: AnimationNodeStateMachine = resolved["machine"]
    var rows: Array = []
    for name in machine.get_node_list():
        var node: AnimationNode = machine.get_node(name)
        var row := {
            "name": str(name),
            "class": node.get_class(),
            "position": _vec2(machine.get_node_position(name)),
        }
        if node is AnimationNodeAnimation:
            row["animation"] = str(node.animation)
        rows.append(row)
    var transitions: Array = []
    for i in machine.get_transition_count():
        var transition: AnimationNodeStateMachineTransition = machine.get_transition(i)
        transitions.append({
            "index": i,
            "from": str(machine.get_transition_from(i)),
            "to": str(machine.get_transition_to(i)),
            "advance_condition": str(transition.advance_condition),
            "advance_expression": transition.advance_expression,
            "advance_mode": transition.advance_mode,
            "priority": transition.priority,
            "reset": transition.reset,
            "switch_mode": transition.switch_mode,
            "xfade_time": transition.xfade_time,
        })
    return {"stamp": ctx._stamp(), "data": {
        "tree": str(args.get("tree", "")),
        "active": resolved["tree"].active,
        "nodes": rows,
        "transitions": transitions,
    }}
static func state_add(ctx, args: Dictionary) -> Dictionary:
    var resolved = _state_machine(ctx, args)
    if resolved.has("_error"):
        return resolved
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty():
        return conflict
    var machine: AnimationNodeStateMachine = resolved["machine"]
    var name := str(args.get("name", ""))
    var kind := str(args.get("kind", ""))
    if name.is_empty() or not NODE_KINDS.has(kind) or machine.has_node(StringName(name)):
        return ctx._error("invalid_argument", "invalid or duplicate AnimationTree state")
    var node_class: String = NODE_KINDS[kind]
    var node = ClassDB.instantiate(node_class)
    if not (node is AnimationNode):
        return ctx._error("backend_failed", "failed to instantiate animation node")
    if node is AnimationNodeAnimation and args.has("animation"):
        node.animation = StringName(str(args["animation"]))
    var pos := args.get("position", [0.0, 0.0])
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [name], "dry-run")
    machine.add_node(StringName(name), node, Vector2(float(pos[0]), float(pos[1])))
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [name], "Add AnimationTree state")

static func state_remove(ctx, args: Dictionary) -> Dictionary:
    var resolved = _state_machine(ctx, args)
    if resolved.has("_error"):
        return resolved
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty():
        return conflict
    var machine: AnimationNodeStateMachine = resolved["machine"]
    var name := str(args.get("name", ""))
    if not machine.has_node(StringName(name)):
        return ctx._error("not_found", "AnimationTree state not found")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [name], "dry-run")
    machine.remove_node(StringName(name))
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [name], "Remove AnimationTree state")
static func transition_add(ctx, args: Dictionary) -> Dictionary:
    var resolved = _state_machine(ctx, args)
    if resolved.has("_error"):
        return resolved
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty():
        return conflict
    var machine: AnimationNodeStateMachine = resolved["machine"]
    var from := str(args.get("from", ""))
    var to := str(args.get("to", ""))
    if not machine.has_node(StringName(from)) or not machine.has_node(StringName(to)):
        return ctx._error("not_found", "AnimationTree transition endpoint not found")
    if machine.has_transition(StringName(from), StringName(to)):
        return ctx._error("conflict", "AnimationTree transition already exists")
    var transition := AnimationNodeStateMachineTransition.new()
    _configure_transition(transition, args)
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [from + "->" + to], "dry-run")
    machine.add_transition(StringName(from), StringName(to), transition)
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [from + "->" + to], "Add AnimationTree transition")

static func transition_configure(ctx, args: Dictionary) -> Dictionary:
    var resolved = _state_machine(ctx, args)
    if resolved.has("_error"):
        return resolved
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty():
        return conflict
    var machine: AnimationNodeStateMachine = resolved["machine"]
    var idx := _transition_index(machine, str(args.get("from", "")), str(args.get("to", "")))
    if idx < 0:
        return ctx._error("not_found", "AnimationTree transition not found")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, ["transition:%d" % idx], "dry-run")
    _configure_transition(machine.get_transition(idx), args)
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, ["transition:%d" % idx], "Configure AnimationTree transition")
static func transition_remove(ctx, args: Dictionary) -> Dictionary:
    var resolved = _state_machine(ctx, args)
    if resolved.has("_error"):
        return resolved
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty():
        return conflict
    var machine: AnimationNodeStateMachine = resolved["machine"]
    var from := str(args.get("from", ""))
    var to := str(args.get("to", ""))
    if not machine.has_transition(StringName(from), StringName(to)):
        return ctx._error("not_found", "AnimationTree transition not found")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [from + "->" + to], "dry-run")
    machine.remove_transition(StringName(from), StringName(to))
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [from + "->" + to], "Remove AnimationTree transition")

static func parameter_set(ctx, args: Dictionary) -> Dictionary:
    var tree = ctx._resolve_node(str(args.get("tree", "")))
    if not (tree is AnimationTree):
        return ctx._error("not_found", "AnimationTree not found")
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty():
        return conflict
    var parameter := str(args.get("parameter", ""))
    if parameter.is_empty() or parameter.contains("..") or parameter.begins_with("/"):
        return ctx._error("invalid_argument", "invalid AnimationTree parameter")
    var property := "parameters/" + parameter
    var exists := false
    for meta in tree.get_property_list():
        if str(meta.get("name", "")) == property:
            exists = true
            break
    if not exists:
        return ctx._error("not_found", "AnimationTree parameter does not exist")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [property], "dry-run")
    tree.set(property, ctx._decode_value(args.get("value")))
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [property], "Set AnimationTree parameter")
static func _state_machine(ctx, args: Dictionary):
    var tree = ctx._resolve_node(str(args.get("tree", "")))
    if not (tree is AnimationTree):
        return ctx._error("not_found", "AnimationTree not found")
    if not (tree.tree_root is AnimationNodeStateMachine):
        return ctx._error("invalid_argument", "AnimationTree root is not a state machine")
    return {"tree": tree, "machine": tree.tree_root}

static func _transition_index(machine: AnimationNodeStateMachine, from: String, to: String) -> int:
    for i in machine.get_transition_count():
        if str(machine.get_transition_from(i)) == from and str(machine.get_transition_to(i)) == to:
            return i
    return -1

static func _configure_transition(transition: AnimationNodeStateMachineTransition, args: Dictionary) -> void:
    if args.has("advance_condition"): transition.advance_condition = StringName(str(args["advance_condition"]))
    if args.has("advance_expression"): transition.advance_expression = str(args["advance_expression"])
    if args.has("advance_mode"): transition.advance_mode = int(args["advance_mode"])
    if args.has("priority"): transition.priority = int(args["priority"])
    if args.has("reset"): transition.reset = bool(args["reset"])
    if args.has("switch_mode"): transition.switch_mode = int(args["switch_mode"])
    if args.has("xfade_time"): transition.xfade_time = float(args["xfade_time"])

static func _vec2(value: Vector2) -> Array:
    return [value.x, value.y]
