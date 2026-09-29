extends Node
# Distributed runtime support. No editor bridge, secrets, filesystem or network API.
var sw_fault: String = ""
var sw_ticks: int = 0
var sw_events: int = 0
var sw_state: String = ""
var _sw_limits: Dictionary = {}
var _sw_event_actions: int = 0
var _sw_queue: Array[String] = []

func _sw_fail(reason: String) -> bool:
    if sw_fault.is_empty():
        sw_fault = reason
        push_error("SEMWRIGHT_IR_FAULT:" + reason)
    set_process(false)
    set_physics_process(false)
    set_process_unhandled_input(false)
    return false

func _sw_spend(cost: int, event: bool = false) -> bool:
    if not sw_fault.is_empty(): return false
    var frame: int = Engine.get_physics_frames()
    var ledger: Dictionary = get_tree().get_meta("semwright_ir_budget", {})
    if ledger.get("frame", -1) != frame:
        ledger = {"frame": frame, "actions": 0, "events": 0, "spawns": 0}
    if ledger.actions + cost > _sw_limits.tick: return _sw_fail("tick_budget")
    if event and ledger.events + 1 > _sw_limits.events: return _sw_fail("event_storm")
    ledger.actions += cost
    if event: ledger.events += 1
    get_tree().set_meta("semwright_ir_budget", ledger)
    return true

func _sw_begin() -> bool:
    _sw_event_actions = 0
    if not _sw_spend(0, true): return false
    sw_events += 1
    return true

func _sw_step() -> bool:
    _sw_event_actions += 1
    if _sw_event_actions > _sw_limits.event: return _sw_fail("event_budget")
    return _sw_spend(1)

func _sw_binding(node: Node) -> bool:
    if not is_instance_valid(node) or node.is_queued_for_deletion():
        return _sw_fail("binding_lost")
    return true

func _sw_valid(v: Variant) -> bool:
    if v is bool: return true
    if v is int or v is float: return is_finite(float(v)) and absf(float(v)) <= 1.0e9
    if v is Vector2: return v.is_finite() and absf(v.x) <= 1.0e9 and absf(v.y) <= 1.0e9
    if v is Vector3: return v.is_finite() and absf(v.x) <= 1.0e9 and absf(v.y) <= 1.0e9 and absf(v.z) <= 1.0e9
    if v is Color: return is_finite(v.r) and is_finite(v.g) and is_finite(v.b) and is_finite(v.a)
    return false

func _sw_emit(signal_id: String) -> void:
    if _sw_queue.size() >= _sw_limits.events: _sw_fail("signal_queue_budget")
    else: _sw_queue.append(signal_id)

func _sw_spawn(prefab: PackedScene, count: int) -> void:
    if not _sw_spend(0): return
    var ledger: Dictionary = get_tree().get_meta("semwright_ir_budget")
    if ledger.spawns + count > _sw_limits.spawns:
        _sw_fail("spawn_rate")
        return
    ledger.spawns += count
    get_tree().set_meta("semwright_ir_budget", ledger)
    for _i in range(count):
        var instance: Node = prefab.instantiate()
        var pending: Array[Node] = [instance]
        var observed: int = 0
        while not pending.is_empty():
            var current: Node = pending.pop_back()
            observed += 1
            if observed + get_tree().get_node_count() > _sw_limits.entities:
                instance.free()
                _sw_fail("entity_budget")
                return
            for child in current.get_children(): pending.append(child)
        get_tree().current_scene.add_child(instance)
        if not sw_fault.is_empty(): return

func _sw_animation_start(tree: AnimationTree, state: StringName) -> bool:
    if not _sw_binding(tree): return false
    var playback: AnimationNodeStateMachinePlayback = tree.get("parameters/playback") as AnimationNodeStateMachinePlayback
    if playback == null: return _sw_fail("animation_state_machine_playback")
    playback.start(state, true)
    return true

func _sw_animation_state(tree: AnimationTree, state: StringName) -> bool:
    if not _sw_binding(tree): return false
    var playback: AnimationNodeStateMachinePlayback = tree.get("parameters/playback") as AnimationNodeStateMachinePlayback
    if playback == null: return _sw_fail("animation_state_machine_playback")
    playback.travel(state, true)
    return true

func _sw_animation_blend(tree: AnimationTree, blend: float) -> bool:
    if not _sw_binding(tree) or not _sw_valid(blend): return false
    tree.set("parameters/blend_position", blend)
    return true

func _sw_rotation(node: Node, value: Variant) -> bool:
    if not _sw_binding(node) or not _sw_valid(value): return false
    if node is Node2D:
        if not (value is int or value is float): return _sw_fail("rotation_type")
        node.rotation = float(value)
        return true
    if node is Node3D:
        if not value is Vector3: return _sw_fail("rotation_type")
        node.rotation = value
        return true
    return _sw_fail("rotation_target")

func _sw_scale(node: Node, value: Variant) -> bool:
    if not _sw_binding(node) or not _sw_valid(value): return false
    if node is Node2D:
        if not value is Vector2: return _sw_fail("scale_type")
        node.scale = value
        return true
    if node is Node3D:
        if not value is Vector3: return _sw_fail("scale_type")
        if is_zero_approx(value.x) or is_zero_approx(value.y) or is_zero_approx(value.z):
            return _sw_fail("scale_zero_component")
        var all_positive: bool = value.x > 0.0 and value.y > 0.0 and value.z > 0.0
        var all_negative: bool = value.x < 0.0 and value.y < 0.0 and value.z < 0.0
        if not (all_positive or all_negative): return _sw_fail("scale_mixed_sign")
        node.scale = value
        return true
    return _sw_fail("scale_target")

func _sw_reparent(node: Node, parent: Node, keep_global: bool) -> bool:
    if not _sw_binding(node) or not _sw_binding(parent): return false
    if node == parent or node.is_ancestor_of(parent): return _sw_fail("reparent_cycle")
    if node.get_tree() != parent.get_tree(): return _sw_fail("reparent_tree")
    if keep_global:
        if node is Node2D:
            if not parent is Node2D: return _sw_fail("reparent_dimension")
        elif node is Node3D:
            if not parent is Node3D: return _sw_fail("reparent_dimension")
        else:
            return _sw_fail("reparent_keep_global_target")
    node.reparent(parent, keep_global)
    return true
