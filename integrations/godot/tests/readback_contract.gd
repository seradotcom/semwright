extends SceneTree
# Synthetic native-object regression tests; never artistic acceptance evidence.
const Readback = preload("res://addons/semwright/ops/readback_ops.gd")
const AnimationOps = preload("res://addons/semwright/ops/animation_ops.gd")
const Codec = preload("res://addons/semwright/ops/variant_codec.gd")
class Context extends RefCounted:
    func _error(code: String, message: String) -> Dictionary:
        return {"_error":code,"message":message}
    func _stamp() -> Dictionary:
        return {"revision":0,"fingerprint":"a".repeat(64)}
    func _encode_value(value):
        return Codec.encode(self, value)
    func _safe_res(path: String) -> String:
        return path
var failures: Array[String] = []
func check(condition: bool, label: String) -> void:
    if not condition: failures.append(label)
func _initialize() -> void:
    call_deferred("run")
func run() -> void:
    var ctx := Context.new()
    var scene := Node3D.new()
    root.add_child(scene)
    scene.position = Vector3(10,20,30)
    var mesh_node := MeshInstance3D.new()
    scene.add_child(mesh_node)
    mesh_node.owner = scene
    mesh_node.position = Vector3(1,2,3)
    var mesh := ArrayMesh.new()
    for vertices in [PackedVector3Array([Vector3(-2,0,1),Vector3(0,4,0),Vector3(1,0,-3)]), PackedVector3Array([Vector3(-5,2,0),Vector3(7,1,2),Vector3(0,-6,8)])]:
        var arrays := []
        arrays.resize(Mesh.ARRAY_MAX)
        arrays[Mesh.ARRAY_VERTEX] = vertices
        mesh.add_surface_from_arrays(Mesh.PRIMITIVE_TRIANGLES, arrays)
    mesh_node.mesh = mesh
    var before := mesh_node.transform
    var observed := Readback.observe(ctx,scene,mesh_node,{"include_vertex_bounds":true})
    check(not observed.has("_error"),"mesh observation succeeds")
    if observed.has("vertex_bounds"):
        var bounds: Dictionary = observed.vertex_bounds
        check(bounds.global_min == [6.0,16.0,30.0],"all surfaces global minimum")
        check(bounds.global_max == [18.0,26.0,41.0],"all surfaces global maximum")
        check(bounds.vertex_count == 6 and bounds.complete,"complete count")
    check(observed.get("owner_path") == "." and observed.get("parent_path") == ".","relations")
    check(mesh_node.transform == before and mesh.get_surface_count() == 2,"readback leaves mesh unchanged")
    var root_read := Readback.observe(ctx,scene,scene,{})
    check(root_read.owner_path == null and root_read.parent_path == null,"root scoped relations")
    var big := ArrayMesh.new()
    var vertices := PackedVector3Array()
    vertices.resize(250001)
    var arrays := []
    arrays.resize(Mesh.ARRAY_MAX)
    arrays[Mesh.ARRAY_VERTEX] = vertices
    big.add_surface_from_arrays(Mesh.PRIMITIVE_POINTS, arrays)
    mesh_node.mesh = big
    var lightweight := Readback.observe(ctx,scene,mesh_node,{})
    check(not lightweight.has("_error") and not lightweight.has("vertex_bounds"),"large mesh retains lightweight default inspection")
    check(Readback.observe(ctx,scene,mesh_node,{"include_vertex_bounds":false}) == lightweight,"explicit false preserves lightweight inspection")
    for invalid_flag in [1,"true",null]:
        check(Readback.observe(ctx,scene,mesh_node,{"include_vertex_bounds":invalid_flag}).has("_error"),"invalid bounds flag rejected")
    var rejected := Readback.observe(ctx,scene,mesh_node,{"include_vertex_bounds":true})
    check(rejected.has("_error") and not rejected.has("vertex_bounds"),"over-budget mesh has no partial success")
    mesh_node.mesh = null
    var empty := Readback.observe(ctx,scene,mesh_node,{"include_vertex_bounds":true})
    check(empty.vertex_bounds.vertex_count == 0 and empty.vertex_bounds.global_min == null,"empty mesh explicit")
    var bad_material := StandardMaterial3D.new()
    bad_material.set_meta("bad_vector",Vector3(NAN,0,0))
    mesh_node.material_override = bad_material
    var bad_attached := Readback.observe(ctx,scene,mesh_node,{"resource_properties":["material_override"]})
    check(bad_attached.has("_error") and not bad_attached.has("resource_properties"),"nonfinite attached resource rejected before wire encoding")
    mesh_node.material_override = null
    var camera := Camera3D.new()
    scene.add_child(camera)
    camera.attributes = CameraAttributesPractical.new()
    camera.attributes.exposure_multiplier = 1.7
    var attached := Readback.observe(ctx,scene,camera,{"resource_properties":["attributes"]})
    check(not attached.has("_error"),"live embedded resource observation")
    if attached.has("resource_properties"):
        check(is_equal_approx(attached.resource_properties.attributes.properties.exposure_multiplier,1.7),"live attribute value")
    camera.attributes.exposure_multiplier = 1.9
    attached = Readback.observe(ctx,scene,camera,{"resource_properties":["attributes"]})
    check(is_equal_approx(attached.resource_properties.attributes.properties.exposure_multiplier,1.9),"current instance not cached loader")
    for selectors in [["script"],["attributes:exposure_multiplier"],["attributes","attributes"]]:
        check(Readback.observe(ctx,scene,camera,{"resource_properties":selectors}).has("_error"),"deny selector " + str(selectors))
    for light in [DirectionalLight3D.new(),OmniLight3D.new(),SpotLight3D.new()]:
        scene.add_child(light)
        if light is DirectionalLight3D:
            light.light_angular_distance = 7.25
        else:
            light.light_size = 0.625
        var light_before: float = light.get_param(Light3D.PARAM_SIZE)
        var light_read := Readback.observe(ctx,scene,light,{})
        check(not light_read.has("_error") and light_read.has("light_size"),"typed light_size exists")
        if light_read.has("light_size"):
            check(is_equal_approx(light_read.light_size,light_before),"light getter matches native PARAM_SIZE")
            if light is DirectionalLight3D:
                check(is_equal_approx(light_read.light_size,light.light_angular_distance),"directional angular alias verified natively")
            else:
                check(is_equal_approx(light_read.light_size,0.625),"positional light size")
        check(light.get_param(Light3D.PARAM_SIZE) == light_before,"light read is inert")
    var player := AnimationPlayer.new()
    scene.add_child(player)
    player.speed_scale = 0
    var animation := Animation.new()
    animation.length = 18
    for track in range(6):
        var index := animation.add_track(Animation.TYPE_VALUE)
        animation.track_set_path(index,NodePath("Camera:position"))
        for key in range(540): animation.track_insert_key(index,key/30.0,float(track*1000+key))
    var library := AnimationLibrary.new()
    library.add_animation("reveal",animation)
    player.add_animation_library("presentation",library)
    var page := AnimationOps.inspect_player(ctx,player,{})
    check(not page.has("_error") and page.get("data",{}).get("returned_keys") == 96,"bounded default page")
    var metadata := AnimationOps.inspect_player(ctx,player,{"keys_limit":0})
    check(metadata.data.returned_keys == 0 and metadata.data.libraries[0].animations[0].tracks.size() == 6,"metadata includes every track")
    var wire_args: Dictionary = JSON.parse_string(JSON.stringify({"keys_offset":0,"keys_limit":32}))
    var wire_page := AnimationOps.inspect_player(ctx,player,wire_args)
    check(not wire_page.has("_error") and wire_page.get("data",{}).get("returned_keys") == 192,"serialized JSON numeric page succeeds")
    check(AnimationOps.inspect_player(ctx,player,{"keys_offset":1000000.0,"keys_limit":0.0}).get("data",{}).get("returned_keys") == 0,"integral float boundaries accepted")
    for field in ["keys_offset","keys_limit"]:
        var invalid_values: Array = [-1,-0.5,0.5,true,false,"0",null,NAN,INF,-INF]
        invalid_values.append(1000001 if field == "keys_offset" else 65)
        for invalid in invalid_values:
            var bad := {"keys_offset":0,"keys_limit":16}
            bad[field] = invalid
            var bad_result := AnimationOps.inspect_player(ctx,player,bad)
            check(bad_result.has("_error") and not bad_result.has("data"),"reject invalid page field before coercion: " + field + "=" + str(invalid))
    var fractional_wire: Dictionary = JSON.parse_string('{"keys_offset":0.5,"keys_limit":32}')
    check(AnimationOps.inspect_player(ctx,player,fractional_wire).has("_error"),"wire fraction rejected")
    var boolean_wire: Dictionary = JSON.parse_string('{"keys_offset":false,"keys_limit":32}')
    check(AnimationOps.inspect_player(ctx,player,boolean_wire).has("_error"),"wire boolean rejected")
    var collected := 0
    for offset in range(0,540,32):
        var serialized_page: Dictionary = JSON.parse_string(JSON.stringify({"keys_offset":offset,"keys_limit":32}))
        page = AnimationOps.inspect_player(ctx,player,serialized_page)
        check(not page.has("_error"),"page succeeds")
        if page.has("_error"): continue
        for track in page.data.libraries[0].animations[0].tracks:
            check(track.key_count == 540,"full key count")
            for key in track.keys:
                check(key.index >= offset and key.index < mini(offset+32,540),"page index")
                check(key.value == float(track.index*1000+key.index),"original key value")
                collected += 1
    check(collected == 3240,"all keys covered once")
    check(AnimationOps.inspect_player(ctx,player,{"keys_limit":64}).has("_error"),"global page budget fail closed")
    check(AnimationOps.inspect_player(ctx,player,{"keys_offset":-1}).has("_error"),"invalid offset rejected")
    check(not player.is_playing() and player.speed_scale == 0 and animation.track_get_key_count(0) == 540,"animation read remains inert")
    var invalid_basis := Basis.IDENTITY
    invalid_basis.x.x = INF
    for invalid_key in [Vector3(NAN,0,0),Color(1,INF,0,1),Transform3D(invalid_basis,Vector3.ZERO),PackedFloat32Array([NAN]),{"nested":[Vector3(INF,0,0)]}]:
        animation.track_set_key_value(0,0,invalid_key)
        var invalid_page := AnimationOps.inspect_player(ctx,player,JSON.parse_string('{"keys_offset":0,"keys_limit":1}'))
        check(invalid_page.has("_error") and not invalid_page.has("data"),"nonfinite key rejects entire page")
    animation.track_set_key_value(0,0,0.0)
    var valid_page := AnimationOps.inspect_player(ctx,player,{"keys_limit":1})
    var response_json := JSON.new()
    check(response_json.parse(JSON.stringify(valid_page)) == OK,"successful page serializes as JSON")
    if response_json.data is Dictionary:
        check(response_json.data.data.returned_keys == 6,"response JSON roundtrip retains complete page")
    scene.free()
    if failures.is_empty():
        print("READBACK_FIXTURE_TESTS PASS: native mesh, relations, live resources, Light3D aliases, JSON pagination, limits, no mutations")
        quit(0)
    else:
        for failure in failures: push_error(failure)
        quit(1)
