extends SceneTree
# Declared synthetic input. Product SaveOps performs the tested mutation;
# product ReadbackOps observes it. No production authoring/E2E claim is made.
const SaveOps = preload("res://addons/semwright/ops/scene_save_ops.gd")
const ReadbackOps = preload("res://addons/semwright/ops/readback_ops.gd")
class Context extends RefCounted:
    func _safe_res(path: String) -> bool:
        return path.begins_with("res://data/") and not path.contains("..") and not path.contains("\\")
    func _error(code: String, message: String) -> Dictionary:
        return {"_error":message,"_code":code}
func _initialize() -> void:
    call_deferred("run")
func emit_receipt(value: Dictionary, phase: String) -> void:
    var file := FileAccess.open("res://" + phase + ".json", FileAccess.WRITE)
    if file == null:
        push_error("receipt cannot be opened")
        quit(1)
        return
    file.store_string(JSON.stringify(value))
    file.close()
func project_state(ctx, scene: Node) -> Dictionary:
    var hero: MeshInstance3D = scene.get_node("Anchor/Hero")
    var other: MeshInstance3D = scene.get_node("Other")
    var player: AnimationPlayer = scene.get_node("Player")
    var observed: Dictionary = ReadbackOps.observe(ctx,scene,hero,{})
    if observed.has("_error"): return observed
    return {"observed":observed,
        "material_path":hero.material_override.resource_path,
        "shared_material":hero.material_override == other.material_override,
        "animation_path":player.get_animation("idle").resource_path}
func run() -> void:
    var phase := str(OS.get_cmdline_user_args()[0])
    var ctx := Context.new()
    var scene: Node3D
    if phase == "write":
        # Input memory contains intended scene state and dirty external resources.
        # The workflow under test is conservative scene-only persistence.
        scene = Node3D.new()
        scene.name = "Root"
        scene.scene_file_path = "res://data/scene.tscn"
        root.add_child(scene)
        var anchor := Node3D.new()
        anchor.name = "Anchor"
        scene.add_child(anchor)
        anchor.owner = scene
        var external: StandardMaterial3D = ResourceLoader.load("res://data/shared.tres","",ResourceLoader.CACHE_MODE_IGNORE)
        var animation: Animation = ResourceLoader.load("res://data/idle.tres","",ResourceLoader.CACHE_MODE_IGNORE)
        if external == null or animation == null:
            push_error("declared fixture resources missing")
            quit(1)
            return
        var hero := MeshInstance3D.new()
        hero.name = "Hero"
        anchor.add_child(hero)
        hero.owner = scene
        hero.position = Vector3(1,2,3)
        hero.material_override = external
        var other := MeshInstance3D.new()
        other.name = "Other"
        scene.add_child(other)
        other.owner = scene
        other.material_override = external
        var player := AnimationPlayer.new()
        player.name = "Player"
        scene.add_child(player)
        player.owner = scene
        var library := AnimationLibrary.new()
        library.add_animation("idle",animation)
        player.add_animation_library("",library)
        external.roughness = 0.2
        animation.length = 0.2
        var result: Dictionary = SaveOps.save_scene_only(ctx,scene,false)
        if result.has("_error") or not result.get("applied",false):
            push_error("native save failed: " + JSON.stringify(result))
            quit(1)
            return
    else:
        var packed: PackedScene = ResourceLoader.load("res://data/scene.tscn","PackedScene",ResourceLoader.CACHE_MODE_IGNORE)
        if packed == null:
            push_error("fresh reopen failed")
            quit(1)
            return
        scene = packed.instantiate()
        root.add_child(scene)
    var projection := project_state(ctx,scene)
    if projection.has("_error"):
        push_error(JSON.stringify(projection))
        quit(1)
        return
    var material: StandardMaterial3D = ResourceLoader.load("res://data/shared.tres","",ResourceLoader.CACHE_MODE_IGNORE)
    var animation: Animation = ResourceLoader.load("res://data/idle.tres","",ResourceLoader.CACHE_MODE_IGNORE)
    emit_receipt({"schema_version":1,"phase":phase,"native_pid":OS.get_process_id(),
        "runtime":Engine.get_version_info().string,"projection":projection,
        "disk_roughness":material.roughness,"disk_animation_length":animation.length},phase)
    scene.free()
    quit(0)
