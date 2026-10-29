extends SceneTree
# Fixed diagnostic consumer. No caller-supplied code, operators or paths.
const INPUT := "res://authoring.glb"
const REPORT := "res://evidence/godot-authoring.json"
const CAPTURE := "res://evidence/godot-authoring.png"
const EXPECTED_POSITION := Vector3(3.0, 1.0, -1.0)
const EPSILON := 0.0001
const EXPECTED := [
    {"id":"tetra", "translation":Vector3(1.0,0.5,-2.0),
     "aabb_position":Vector3(0.0,0.0,-1.0), "aabb_size":Vector3(2.0,3.0,1.0),
     "indices":12, "linear_color":Color(0.45,0.24,0.10,1.0), "roughness":0.7},
    {"id":"wedge", "translation":Vector3(4.0,0.0,0.0),
     "aabb_position":Vector3(0.0,0.0,-2.0), "aabb_size":Vector3(2.0,1.0,2.0),
     "indices":24, "linear_color":Color(0.08,0.32,0.45,1.0), "roughness":0.5}
]
var started := 0
var finished := false
var writable := false
var report: Dictionary = {
	"proof": "two_mesh_authoring_native_godot_consumer",
	"godot_broker_used": false, "godot_windows_driver_qualified": false,
	"art_review": "NOT_APPLICABLE_DIAGNOSTIC_FIXTURE"
}

func _initialize() -> void:
	started = Time.get_ticks_msec()
	root.title = "Semwright — native Godot GLB proof"
	root.size = Vector2i(1280, 720)
	call_deferred("_run")

func _run() -> void:
	if not DirAccess.dir_exists_absolute(ProjectSettings.globalize_path("res://evidence")):
		_finish(2, "Existing evidence directory required")
		return
	if FileAccess.file_exists(REPORT) or FileAccess.file_exists(CAPTURE):
		_finish(2, "Evidence exists; preserve previous attempt")
		return
	writable = true
	create_timer(20.0).timeout.connect(func(): _finish(90, "In-process deadline"))
	report["godot_version"] = Engine.get_version_info()
	report["os"] = OS.get_name()
	report["display_server"] = DisplayServer.get_name()
	report["rendering_method"] = RenderingServer.get_current_rendering_method()
	report["video_adapter_name"] = RenderingServer.get_video_adapter_name()
	report["video_adapter_vendor"] = RenderingServer.get_video_adapter_vendor()
	report["user_data_directory"] = OS.get_user_data_dir()
	var source_hash := FileAccess.get_sha256(INPUT)
	report["input_sha256"] = source_hash
	if source_hash.length() != 64:
		_finish(3, "Input GLB cannot be hashed")
		return
	var packed := load(INPUT) as PackedScene
	if packed == null:
		_finish(3, "Imported GLB is not a PackedScene")
		return
	var world := Node3D.new()
	root.add_child(world)
	var imported := packed.instantiate()
	world.add_child(imported)
	await process_frame
	if finished:
		return
	var validation_error := _check_authoring(imported)
	if not validation_error.is_empty():
		_finish(6, validation_error)
		return
	if DisplayServer.get_name().to_lower() == "headless" or str(RenderingServer.get_current_rendering_method()) != "gl_compatibility":
		_finish(7, "Visible GL Compatibility rendering required")
		return
	report["color_comparison"] = "srgb_to_linear(BaseMaterial3D.albedo_color) vs glTF linear factor"
	var environment := Environment.new()
	environment.background_mode = Environment.BG_COLOR
	environment.background_color = Color(0.07, 0.09, 0.13)
	environment.ambient_light_source = Environment.AMBIENT_SOURCE_COLOR
	environment.ambient_light_color = Color(0.72, 0.80, 0.92)
	environment.ambient_light_energy = 0.45
	var world_environment := WorldEnvironment.new()
	world_environment.environment = environment
	world.add_child(world_environment)
	var light := DirectionalLight3D.new()
	light.rotation_degrees = Vector3(-40.0, -35.0, 0.0)
	light.light_energy = 1.4
	world.add_child(light)
	var camera := Camera3D.new()
	camera.position = Vector3(10.0, 8.0, 11.0)
	camera.fov = 40.0
	camera.far = 100.0
	world.add_child(camera)
	camera.look_at(EXPECTED_POSITION, Vector3.UP)
	camera.make_current()
	report["camera_position"] = _v3(camera.global_position)
	report["camera_target"] = _v3(EXPECTED_POSITION)
	for frame in range(8):
		await process_frame
		if finished:
			return
	await RenderingServer.frame_post_draw
	if finished:
		return
	var image := root.get_texture().get_image()
	if image == null or image.is_empty():
		_finish(7, "Empty native viewport image")
		return
	report["capture_size"] = [image.get_width(), image.get_height()]
	if image.get_width() != 1280 or image.get_height() != 720:
		_finish(7, "Capture size is not 1280x720")
		return
	if FileAccess.get_sha256(INPUT) != source_hash:
		_finish(9, "Input changed during verification")
		return
	if FileAccess.file_exists(CAPTURE) or image.save_png(CAPTURE) != OK:
		_finish(8, "Capture could not be saved without overwrite")
		return
	report["capture_sha256"] = FileAccess.get_sha256(CAPTURE)
	_finish(0, "Native two-mesh geometry, materials, transforms and visible frame verified")

func _collect(node: Node, result: Array[MeshInstance3D]) -> void:
	if node is MeshInstance3D:
		result.append(node as MeshInstance3D)
	for child in node.get_children():
		_collect(child, result)

func _v3(value: Vector3) -> Array:
	return [value.x, value.y, value.z]

func _finish(code: int, message: String) -> void:
	if finished:
		return
	finished = true
	report["status"] = "PASS" if code == 0 else "FAIL"
	report["exit_code"] = code
	report["message"] = message
	report["elapsed_ms"] = Time.get_ticks_msec() - started
	if writable and not FileAccess.file_exists(REPORT):
		var output := FileAccess.open(REPORT, FileAccess.WRITE)
		if output == null:
			code = 91
		else:
			output.store_string(JSON.stringify(report, "\t") + "\n")
			output.flush()
			if output.get_error() != OK:
				code = 91
			output.close()
	print(JSON.stringify(report))
	quit(code)

func _check_authoring(imported: Node) -> String:
	var nodes: Array[MeshInstance3D] = []
	_collect(imported, nodes)
	report["mesh_instance_count"] = nodes.size()
	report["meshes"] = []
	if nodes.size() != 2:
		return "Expected exactly two MeshInstance3D nodes"
	var seen: Dictionary = {}
	for node in nodes:
		var transform := node.global_transform
		var expected: Dictionary = {}
		for candidate in EXPECTED:
			var target: Vector3 = candidate["translation"]
			if transform.origin.is_finite() and transform.origin.distance_to(target) <= EPSILON:
				expected = candidate
				break
		if expected.is_empty():
			return "Mesh has no matching expected translation"
		var identifier: String = expected["id"]
		if seen.has(identifier):
			return "Two meshes matched the same expected translation"
		seen[identifier] = true
		var row: Dictionary = {
			"fixture_id": identifier, "native_name": node.name,
			"path": str(imported.get_path_to(node)),
			"global_translation": _v3(transform.origin),
			"global_scale": _v3(transform.basis.get_scale()),
			"global_basis": [_v3(transform.basis.x), _v3(transform.basis.y), _v3(transform.basis.z)]
		}
		report["meshes"].append(row)
		if not _identity_basis(transform.basis):
			return identifier + ": rotation/scale is not identity"
		if not node.is_visible_in_tree():
			return identifier + ": mesh is not visible"
		var mesh := node.mesh
		if mesh == null or mesh.get_surface_count() != 1:
			return identifier + ": expected one mesh surface"
		if mesh.surface_get_primitive_type(0) != Mesh.PRIMITIVE_TRIANGLES:
			return identifier + ": surface is not a triangle list"
		var arrays := mesh.surface_get_arrays(0)
		if arrays.size() <= Mesh.ARRAY_INDEX:
			return identifier + ": incomplete surface arrays"
		if typeof(arrays[Mesh.ARRAY_VERTEX]) != TYPE_PACKED_VECTOR3_ARRAY or typeof(arrays[Mesh.ARRAY_INDEX]) != TYPE_PACKED_INT32_ARRAY:
			return identifier + ": expected indexed 3D surface"
		var vertices: PackedVector3Array = arrays[Mesh.ARRAY_VERTEX]
		var indices: PackedInt32Array = arrays[Mesh.ARRAY_INDEX]
		row["vertex_count"] = vertices.size()
		row["index_count"] = indices.size()
		row["triangle_count"] = indices.size() / 3
		if vertices.size() < 4 or indices.size() != int(expected["indices"]):
			return identifier + ": geometry counts disagree"
		for vertex in vertices:
			if not vertex.is_finite():
				return identifier + ": non-finite vertex"
		for index in indices:
			if index < 0 or index >= vertices.size():
				return identifier + ": index outside vertex array"
		var bounds := mesh.get_aabb()
		row["local_aabb_position"] = _v3(bounds.position)
		row["local_aabb_size"] = _v3(bounds.size)
		var expected_position: Vector3 = expected["aabb_position"]
		var expected_size: Vector3 = expected["aabb_size"]
		if not bounds.position.is_finite() or not bounds.size.is_finite():
			return identifier + ": non-finite bounds"
		if bounds.position.distance_to(expected_position) > EPSILON or bounds.size.distance_to(expected_size) > EPSILON:
			return identifier + ": local bounds disagree"
		var material := node.get_active_material(0) as BaseMaterial3D
		if material == null:
			return identifier + ": active material is not BaseMaterial3D"
		var raw_color := material.albedo_color
		var linear_color := raw_color.srgb_to_linear()
		var expected_color: Color = expected["linear_color"]
		row["material_name"] = material.resource_name
		row["raw_albedo_property"] = _rgba(raw_color)
		row["albedo_after_srgb_to_linear"] = _rgba(linear_color)
		row["expected_linear_albedo"] = _rgba(expected_color)
		row["roughness"] = material.roughness
		row["metallic"] = material.metallic
		row["linear_color_max_error"] = _color_error(linear_color, expected_color)
		for component in _rgba(linear_color):
			if not is_finite(component):
				return identifier + ": non-finite material"
		if _color_error(linear_color, expected_color) > EPSILON:
			return identifier + ": linear albedo disagrees"
		if not is_finite(material.roughness) or absf(material.roughness - float(expected["roughness"])) > EPSILON:
			return identifier + ": roughness disagrees"
		if not is_finite(material.metallic) or absf(material.metallic) > EPSILON:
			return identifier + ": metallic is not zero"
		if material.albedo_texture != null:
			return identifier + ": unexpected albedo texture"
	return "" if seen.size() == 2 else "Both fixture identities were not recovered"

func _identity_basis(value: Basis) -> bool:
	return value.x.is_finite() and value.y.is_finite() and value.z.is_finite() and value.x.distance_to(Vector3.RIGHT) <= EPSILON and value.y.distance_to(Vector3.UP) <= EPSILON and value.z.distance_to(Vector3.BACK) <= EPSILON

func _rgba(value: Color) -> Array:
	return [value.r, value.g, value.b, value.a]

func _color_error(actual: Color, expected: Color) -> float:
	return maxf(maxf(absf(actual.r - expected.r), absf(actual.g - expected.g)), maxf(absf(actual.b - expected.b), absf(actual.a - expected.a)))
