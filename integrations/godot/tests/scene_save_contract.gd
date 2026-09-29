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
    var texture_path := directory + "/shared_texture.tres"
    write(path,"[gd_scene format=3]\n\n[node name=\"Root\" type=\"Node3D\"]\n")
    var authored_gradient := Gradient.new()
    authored_gradient.colors = PackedColorArray([Color(1,0,0,1),Color(0,0,1,1)])
    var authored_texture := GradientTexture1D.new()
    authored_texture.width = 32
    authored_texture.gradient = authored_gradient
    check(ResourceSaver.save(authored_texture,texture_path) == OK,"save external texture fixture")
    var texture: GradientTexture1D = ResourceLoader.load(texture_path,"GradientTexture1D",ResourceLoader.CACHE_MODE_IGNORE)
    check(texture != null,"load external texture fixture")
    var authored_material := StandardMaterial3D.new()
    authored_material.roughness = 0.7
    authored_material.albedo_texture = texture
    check(ResourceSaver.save(authored_material,material_path) == OK,"save external material fixture")
    var external: StandardMaterial3D = ResourceLoader.load(material_path,"StandardMaterial3D",ResourceLoader.CACHE_MODE_IGNORE)
    check(external != null,"load external material fixture")
    var external_hash := FileAccess.get_sha256(material_path)
    var texture_hash := FileAccess.get_sha256(texture_path)
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
    var embedded_material := StandardMaterial3D.new()
    embedded_material.albedo_color = Color(0.2,0.6,0.9,1.0)
    embedded_material.roughness = 0.33
    var embedded_a := MeshInstance3D.new()
    embedded_a.name = "EmbeddedA"
    scene.add_child(embedded_a)
    embedded_a.owner = scene
    embedded_a.material_override = embedded_material
    var embedded_b := MeshInstance3D.new()
    embedded_b.name = "EmbeddedB"
    scene.add_child(embedded_b)
    embedded_b.owner = scene
    embedded_b.material_override = embedded_material
    hero.reparent(anchor,false)
    hero.owner = scene
    hero.position = Vector3(1,2,3)
    # Deliberate in-memory external edits must NOT be persisted in this explicit mode.
    external.roughness = 0.2
    var live_texture := external.albedo_texture as GradientTexture1D
    check(live_texture != null,"external material resolves external texture")
    if live_texture != null: live_texture.width = 64
    var before := FileAccess.get_sha256(path)
    var dry := SaveOps.save_scene_only(ctx,scene,true)
    check(not dry.has("_error") and not dry.get("applied",true),"dry run validates but does not apply")
    check(FileAccess.get_sha256(path) == before and FileAccess.get_sha256(material_path) == external_hash and FileAccess.get_sha256(texture_path) == texture_hash,"dry run files unchanged")
    var saved := SaveOps.save_scene_only(ctx,scene,false)
    check(not saved.has("_error") and saved.get("applied",false),"scene-only save succeeds")
    check(FileAccess.get_sha256(material_path) == external_hash,"external material bytes exactly preserved")
    check(FileAccess.get_sha256(texture_path) == texture_hash,"external texture bytes exactly preserved")
    check(is_equal_approx(external.roughness,0.2),"live external material edit remains in memory")
    if live_texture != null: check(live_texture.width == 64,"live external texture edit remains in memory")
    var disk: StandardMaterial3D = ResourceLoader.load(material_path,"StandardMaterial3D",ResourceLoader.CACHE_MODE_IGNORE)
    check(disk != null and is_equal_approx(disk.roughness,0.7),"external material on disk retains original value")
    if disk != null:
        check(disk.albedo_texture != null and disk.albedo_texture.resource_path == texture_path,"external texture reference retained by material")
    var disk_texture: GradientTexture1D = ResourceLoader.load(texture_path,"GradientTexture1D",ResourceLoader.CACHE_MODE_IGNORE)
    check(disk_texture != null and disk_texture.width == 32,"external texture on disk retains original value")
    var packed: PackedScene = ResourceLoader.load(path,"PackedScene",ResourceLoader.CACHE_MODE_IGNORE)
    check(packed != null,"saved scene loadable")
    if packed != null:
        var reopened := packed.instantiate()
        check(reopened.has_node("Anchor/Hero") and reopened.has_node("Other"),"hierarchy survives scene reload")
        if reopened.has_node("Anchor/Hero"):
            var loaded: MeshInstance3D = reopened.get_node("Anchor/Hero")
            check(loaded.owner == reopened and loaded.position == Vector3(1,2,3),"ownership and position persisted")
            check(loaded.material_override.resource_path == material_path,"external material reference retained")
            check(loaded.material_override == reopened.get_node("Other").material_override,"external sharing retained")
            check(loaded.material_override.albedo_texture != null and loaded.material_override.albedo_texture.resource_path == texture_path,"external texture reference survives scene reload")
        if reopened.has_node("EmbeddedA") and reopened.has_node("EmbeddedB"):
            var embedded_loaded_a: MeshInstance3D = reopened.get_node("EmbeddedA")
            var embedded_loaded_b: MeshInstance3D = reopened.get_node("EmbeddedB")
            check(embedded_loaded_a.material_override != null,"embedded subresource material survives reload")
            if embedded_loaded_a.material_override != null:
                check(is_equal_approx(embedded_loaded_a.material_override.roughness,0.33),"embedded subresource value survives reload")
                check(embedded_loaded_a.material_override.resource_path.contains("::"),"embedded material remains scene subresource")
                check(embedded_loaded_a.material_override == embedded_loaded_b.material_override,"shared embedded subresource identity retained")
        else:
            failures.append("embedded subresource nodes survive scene reload")
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
        print("SCENE_SAVE_FIXTURE_TESTS PASS: scene-only save, external material/texture bytes, embedded/shared subresources, inherited rejection, ownership, limits")
        quit(0)
    else:
        for failure in failures: push_error(failure)
        quit(1)
