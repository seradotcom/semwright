extends SceneTree
func _initialize() -> void:
    call_deferred("_run")
func _run() -> void:
    var packed := load("res://proofs/semwright_godot_bridge01_02.tscn") as PackedScene
    if packed == null:
        push_error("Persisted supporting scene could not load")
        quit(3)
        return
    var fixture := packed.instantiate()
    root.add_child(fixture)
    var queue: Array[Node] = [fixture]
    var meshes: Array = []
    var camera: Dictionary = {}
    var visited := 0
    while not queue.is_empty():
        var node: Node = queue.pop_back()
        visited += 1
        if visited > 128:
            quit(4)
            return
        for child in node.get_children():
            queue.append(child)
        if node is Camera3D:
            var c := node as Camera3D
            camera = {"path":str(fixture.get_path_to(c)), "fov":c.fov, "near":c.near, "far":c.far,
                      "position":[c.position.x,c.position.y,c.position.z],
                      "rotation":[c.rotation.x,c.rotation.y,c.rotation.z]}
        if node is MeshInstance3D:
            var instance := node as MeshInstance3D
            var mesh := instance.mesh
            if mesh == null:
                quit(5)
                return
            var low := Vector3(INF,INF,INF)
            var high := Vector3(-INF,-INF,-INF)
            var count := 0
            for surface in range(mesh.get_surface_count()):
                var arrays := mesh.surface_get_arrays(surface)
                var vertices: PackedVector3Array = arrays[Mesh.ARRAY_VERTEX]
                for vertex in vertices:
                    var point: Vector3 = instance.global_transform * vertex
                    low = low.min(point)
                    high = high.max(point)
                    count += 1
            if count == 0 or count > 250000:
                quit(6)
                return
            meshes.append({"path":str(fixture.get_path_to(instance)),"name":str(instance.name),
                "surface_count":mesh.get_surface_count(),"vertex_count":count,
                "min":[low.x,low.y,low.z],"max":[high.x,high.y,high.z]})
    var file := FileAccess.open("E:/Projects/worldwright-archipelago/evidence/integration/20261029-godot-broker/godot-bridge01/persisted-fixture-readback01/native-readback.json",FileAccess.WRITE)
    if file == null:
        quit(7)
        return
    file.store_string(JSON.stringify({"engine":Engine.get_version_info(),"scene":"res://proofs/semwright_godot_bridge01_02.tscn",
        "nodes":visited,"meshes":meshes,"camera":camera,"source":"native base mesh vertex arrays"}, "  "))
    file.close()
    quit(0)
