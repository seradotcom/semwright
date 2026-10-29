extends SceneTree

const CONFIG_PATH = "E:/Projects/worldwright-archipelago/evidence/integration/20261029-godot-broker/godot-bridge01/human-camera-persistence01/observer-config.json"
const RESULT_MARKER = "WORLDWRIGHT_CAMERA_PERSISTENCE_V1 "
const NODE_LIMIT = 128
const VERTEX_LIMIT = 250000

var problems: Array[String] = []

func _initialize() -> void:
    call_deferred("_run")

func problem(message: String) -> void:
    problems.append(message)

func encode_property(value: Variant) -> Variant:
    match typeof(value):
        TYPE_NIL:
            return null
        TYPE_BOOL, TYPE_INT, TYPE_STRING:
            return value
        TYPE_FLOAT:
            if not is_finite(value):
                problem("Non-finite scalar camera property")
                return null
            return value
        TYPE_VECTOR2:
            if not value.is_finite():
                problem("Non-finite Vector2 camera property")
                return null
            return {"$type":"Vector2", "value":[value.x, value.y]}
        TYPE_VECTOR3:
            if not value.is_finite():
                problem("Non-finite Vector3 camera property")
                return null
            return {"$type":"Vector3", "value":[value.x, value.y, value.z]}
        TYPE_TRANSFORM3D:
            if not value.is_finite():
                problem("Non-finite Transform3D camera property")
                return null
            return {"$type":"Transform3D", "value":[
                value.basis.x.x, value.basis.x.y, value.basis.x.z,
                value.basis.y.x, value.basis.y.y, value.basis.y.z,
                value.basis.z.x, value.basis.z.y, value.basis.z.z,
                value.origin.x, value.origin.y, value.origin.z]}
        TYPE_NODE_PATH:
            return {"$type":"NodePath", "value":str(value)}
        _:
            problem("Unsupported camera property Variant type: " + str(typeof(value)))
            return null

func scene_is_plain(packed: PackedScene, visited: Dictionary, depth: int) -> bool:
    if depth > 8:
        problem("Nested scene depth exceeds the bounded fixture")
        return false
    var identity := packed.get_instance_id()
    if visited.has(identity):
        return true
    visited[identity] = true
    var state := packed.get_state()
    if state.get_node_count() > NODE_LIMIT or state.get_connection_count() != 0:
        problem("Unexpected node count or signal connections in fixture")
        return false
    for index in range(state.get_node_count()):
        var node_type := str(state.get_node_type(index))
        if node_type not in ["", "Node3D", "MeshInstance3D", "Camera3D"]:
            problem("Unexpected fixture node class: " + node_type)
            return false
        for property_index in range(state.get_node_property_count(index)):
            var name := str(state.get_node_property_name(index, property_index))
            var value: Variant = state.get_node_property_value(index, property_index)
            if (name == "script" and value != null) or value is Script:
                problem("Fixture contains a script; refusing instantiation")
                return false
        var nested: PackedScene = state.get_node_instance(index)
        if nested != null and not scene_is_plain(nested, visited, depth + 1):
            return false
    return true

func emit_result(result: Dictionary) -> void:
    result["errors"] = problems
    result["ok"] = problems.is_empty()
    print(RESULT_MARKER + JSON.stringify(result, "", true, true))
    quit(0 if problems.is_empty() else 3)

func _run() -> void:
    var config: Variant = JSON.parse_string(FileAccess.get_file_as_string(CONFIG_PATH))
    if not config is Dictionary:
        problem("Cannot read observer configuration")
        emit_result({})
        return
    var packed := load(str(config["scene"])) as PackedScene
    if packed == null:
        problem("Pinned supporting scene could not load")
        emit_result({})
        return
    if not scene_is_plain(packed, {}, 0):
        emit_result({})
        return
    var fixture := packed.instantiate()
    if fixture == null:
        problem("Supporting scene could not instantiate")
        emit_result({})
        return
    var queue: Array[Node] = [fixture]
    var nodes: Array[Node] = []
    var paths: Array[String] = []
    var cameras: Dictionary = {}
    var managed: Dictionary = {}
    # Capture authored camera properties BEFORE entering a SceneTree. Entering
    # a viewport may select a current camera even when its stored flag is false.
    while not queue.is_empty():
        var node: Node = queue.pop_back()
        if nodes.size() >= NODE_LIMIT:
            problem("Fixture node limit exceeded")
            break
        nodes.append(node)
        var path := str(fixture.get_path_to(node))
        paths.append(path)
        if node.get_script() != null:
            problem("Instantiated fixture node unexpectedly has a script")
            break
        for child in node.get_children():
            queue.append(child)
        if path == str(config["managed_target_path"]):
            if not node is Node3D:
                problem("Managed target is not Node3D")
                break
            var target := node as Node3D
            managed = {"path":path, "position":[target.position.x, target.position.y, target.position.z]}
        if node is Camera3D:
            if not config["camera_keys"].has(path):
                problem("Unexpected camera: " + path)
                break
            var available: Dictionary = {}
            for descriptor in node.get_property_list():
                available[str(descriptor["name"])] = true
            var properties: Dictionary = {}
            for key in config["camera_keys"][path]:
                if not available.has(str(key)):
                    problem("Missing camera property: " + path + "." + str(key))
                    break
                properties[str(key)] = encode_property(node.get(str(key)))
            cameras[path] = properties
    paths.sort()
    var result: Dictionary = {
        "scene":str(config["scene"]), "engine":Engine.get_version_info(),
        "node_paths":paths, "node_count":nodes.size(), "cameras":cameras,
        "camera_observation_phase":"before_add_child",
        "managed_target":managed, "meshes":[],
        "geometry_source":"native base mesh vertex arrays in global space"}
    if not problems.is_empty():
        fixture.free()
        emit_result(result)
        return
    root.add_child(fixture)
    var meshes: Array = []
    var total_vertices := 0
    for node in nodes:
        if not node is MeshInstance3D:
            continue
        var instance := node as MeshInstance3D
        var mesh := instance.mesh
        if mesh == null:
            problem("Native mesh resource is absent")
            break
        instance.force_update_transform()
        var low := Vector3(INF, INF, INF)
        var high := Vector3(-INF, -INF, -INF)
        var count := 0
        for surface in range(mesh.get_surface_count()):
            var arrays := mesh.surface_get_arrays(surface)
            if arrays.size() <= Mesh.ARRAY_VERTEX or typeof(arrays[Mesh.ARRAY_VERTEX]) != TYPE_PACKED_VECTOR3_ARRAY:
                problem("Native vertex arrays are unavailable")
                break
            var vertices: PackedVector3Array = arrays[Mesh.ARRAY_VERTEX]
            for vertex in vertices:
                var point: Vector3 = instance.global_transform * vertex
                if not point.is_finite():
                    problem("Non-finite native vertex")
                    break
                low = low.min(point)
                high = high.max(point)
                count += 1
                total_vertices += 1
                if total_vertices > VERTEX_LIMIT:
                    problem("Native geometry observation exceeds vertex budget")
                    break
            if not problems.is_empty():
                break
        if count == 0 or not problems.is_empty():
            if count == 0:
                problem("Native mesh has no vertices")
            break
        meshes.append({"path":str(fixture.get_path_to(instance)), "name":str(instance.name),
            "surface_count":mesh.get_surface_count(), "vertex_count":count,
            "min":[low.x, low.y, low.z], "max":[high.x, high.y, high.z]})
    result["meshes"] = meshes
    result["observed_vertices"] = total_vertices
    fixture.free()
    emit_result(result)
