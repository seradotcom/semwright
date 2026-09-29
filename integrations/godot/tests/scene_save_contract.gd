extends SceneTree
# Synthetic fixture only. All writes use a unique user:// directory, no art files.
const SaveOps = preload("res://addons/semwright/ops/scene_save_ops.gd")
class Context extends RefCounted:
    var allowed_prefix: String
    func _safe_res(path: String) -> bool:
        return path.begins_with(allowed_prefix + "/") and not path.contains("..") and not path.contains("\\")
    func _error(code: String, message: String) -> Dictionary:
        return {"_error":message,"_code":code}
var failures: Array[String] = []
func check(condition: bool, label: String) -> void:
    if not condition: failures.append(label)
func write(path: String, text: String) -> void:
    var file := FileAccess.open(path,FileAccess.WRITE)
    if file == null:
        failures.append("cannot write fixture: " + path)
        return
    file.store_string(text)
    file.close()
func _initialize() -> void:
    call_deferred("run")
func run() -> void:
    var directory := "user://scene-save-fixture-%s-%s" % [OS.get_process_id(),Time.get_ticks_usec()]
    check(DirAccess.make_dir_recursive_absolute(directory) == OK,"unique fixture directory")
    var ctx := Context.new()
    ctx.allowed_prefix = directory
    var path := directory + "/scene.tscn"
    var material_path := directory + "/shared.tres"
    write(path,"[gd_scene format=3]\n\n[node name=\"Root\" type=\"Node3D\"]\n")
    write(material_path,"[gd_resource type=\"StandardMaterial3D\" format=3]\n\n[resource]\nroughness = 0.7\n")
    var external: StandardMaterial3D = ResourceLoader.load(material_path,"",ResourceLoader.CACHE_MODE_IGNORE)
    check(external != null,"load external fixture")
    var external_hash := FileAccess.get_sha256(material_path)
    var scene := Node3D.new()
    scene.name = "Root"
    scene.scene_file_path = path
    root.add_child(scene)
    var anchor := Node3D.new()
    anchor.name = "Anchor"
    scene.add_child(anchor)
    anchor.owner = scene
    var hero := MeshInstance3D.new()
    hero.name = "Hero"
    scene.add_child(hero)
    hero.owner = scene
    hero.material_override = external
    var other := MeshInstance3D.new()
    other.name = "Other"
    scene.add_child(other)
    other.owner = scene
    other.material_override = external
    hero.reparent(anchor,false)
    hero.owner = scene
    hero.position = Vector3(1,2,3)
    # Deliberate in-memory external edit must NOT be persisted in this explicit mode.
    external.roughness = 0.2
    var before := FileAccess.get_sha256(path)
    var dry := SaveOps.save_scene_only(ctx,scene,true)
    check(not dry.has("_error") and not dry.get("applied",true),"dry run validates but does not apply")
    check(FileAccess.get_sha256(path) == before and FileAccess.get_sha256(material_path) == external_hash,"dry run files unchanged")
    var saved := SaveOps.save_scene_only(ctx,scene,false)
    check(not saved.has("_error") and saved.get("applied",false),"scene-only save succeeds")
    check(FileAccess.get_sha256(material_path) == external_hash,"external bytes exactly preserved")
    check(is_equal_approx(external.roughness,0.2),"live external edit remains in memory")
    var disk: StandardMaterial3D = ResourceLoader.load(material_path,"",ResourceLoader.CACHE_MODE_IGNORE)
    check(is_equal_approx(disk.roughness,0.7),"external on disk retains original value")
    var packed: PackedScene = ResourceLoader.load(path,"PackedScene",ResourceLoader.CACHE_MODE_IGNORE)
    check(packed != null,"saved scene loadable")
    if packed != null:
        var reopened := packed.instantiate()
        check(reopened.has_node("Anchor/Hero") and reopened.has_node("Other"),"hierarchy survives scene reload")
        if reopened.has_node("Anchor/Hero"):
            var loaded: MeshInstance3D = reopened.get_node("Anchor/Hero")
            check(loaded.owner == reopened and loaded.position == Vector3(1,2,3),"ownership and position persisted")
            check(loaded.material_override.resource_path == material_path,"external resource reference retained")
            check(loaded.material_override == reopened.get_node("Other").material_override,"sharing retained")
        reopened.free()
    before = FileAccess.get_sha256(path)
    hero.owner = null
    var rejected := SaveOps.save_scene_only(ctx,scene,false)
    check(rejected.get("_code") == "unsupported" and FileAccess.get_sha256(path) == before,"unowned descendant rejected before write")
    hero.owner = scene
    hero.scene_file_path = directory + "/nested.tscn"
    rejected = SaveOps.save_scene_only(ctx,scene,false)
    check(rejected.get("_code") == "unsupported" and FileAccess.get_sha256(path) == before,"instance descendant rejected before write")
    hero.scene_file_path = ""
    scene.scene_file_path = directory + "/../escape.tscn"
    check(SaveOps.save_scene_only(ctx,scene,false).get("_code") == "invalid_argument","traversal rejected")
    scene.scene_file_path = directory + "/missing.tscn"
    check(SaveOps.save_scene_only(ctx,scene,false).get("_code") == "not_found","new target path rejected")
    scene.scene_file_path = path
    var inherited_path := directory + "/inherited.tscn"
    write(inherited_path,"[gd_scene load_steps=2 format=3]\n\n[ext_resource type=\"PackedScene\" path=\"" + path + "\" id=\"1\"]\n\n[node name=\"Inherited\" instance=ExtResource(\"1\")]\n")
    var inherited_packed: PackedScene = ResourceLoader.load(inherited_path,"PackedScene",ResourceLoader.CACHE_MODE_IGNORE)
    check(inherited_packed != null,"inherited fixture loadable")
    if inherited_packed != null:
        var inherited := inherited_packed.instantiate(PackedScene.GEN_EDIT_STATE_MAIN)
        root.add_child(inherited)
        var inherited_hash := FileAccess.get_sha256(inherited_path)
        rejected = SaveOps.save_scene_only(ctx,inherited,false)
        check(rejected.get("_code") == "unsupported","inherited scene rejected")
        check(FileAccess.get_sha256(inherited_path) == inherited_hash,"inherited file unchanged")
        inherited.free()
    var extras: Array[Node] = []
    for i in range(SaveOps.MAX_NODES):
        var child := Node.new()
        scene.add_child(child)
        child.owner = scene
        extras.append(child)
    check(SaveOps.save_scene_only(ctx,scene,false).get("_code") == "resource_exhausted","node budget rejected")
    check(FileAccess.get_sha256(path) == before,"failed checks did not write scene")
    scene.free()
    print("SCENE_SAVE_FIXTURE_DIRECTORY ",ProjectSettings.globalize_path(directory))
    if failures.is_empty():
        print("SCENE_SAVE_FIXTURE_TESTS PASS: scene-only save, dry run, sharing, ownership, external bytes, limits, rejection")
        quit(0)
    else:
        for failure in failures: push_error(failure)
        quit(1)
